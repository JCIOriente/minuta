//! External process runner and toolchain preflight. Tools are always run from an argv
//! array, never through a shell.

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::{Error, Missing, Result};

const STDERR_TAIL_LINES: usize = 20;
/// How long a `--version` probe may take before the tool counts as broken.
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// Options for [`run`].
#[derive(Debug, Default, Clone)]
pub struct RunOpts {
    /// Working directory of the child; inherited when `None`.
    pub cwd: Option<PathBuf>,
    /// When set to `true` by the caller, the child is killed and `run` returns
    /// `Error::Interrupted`.
    pub interrupt: Option<Arc<AtomicBool>>,
}

/// Result of a successful [`run`].
#[derive(Debug, Clone, Copy)]
pub struct Output {
    pub status: ExitStatus,
}

/// One line of a tool's output.
struct Piece {
    text: String,
    /// It ended in a bare `\r`: a progress update (ffmpeg's stats) that the next one
    /// overwrites on a terminal.
    update: bool,
}

type Line = (bool, std::io::Result<Piece>);

fn piece(bytes: &mut Vec<u8>, update: bool) -> Piece {
    let text = String::from_utf8_lossy(bytes).into_owned();
    bytes.clear();
    Piece { text, update }
}

/// Forward lines until EOF. A line ends at `\n`, `\r\n` or a bare `\r`, so `\r`-separated
/// progress updates arrive as they are written. Bytes are decoded lossily so non-UTF-8
/// output never stops the reader; a real I/O error is forwarded to the caller and ends it.
fn pump<R: Read + Send + 'static>(
    stream: R,
    is_stderr: bool,
    tx: mpsc::Sender<Line>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = Vec::new();
        // The last chunk ended in `\r`, already sent as an update; a `\n` that starts the
        // next chunk belongs to it. (Such a split `\r\n` line is then counted as an update,
        // which only affects how the stderr tail keeps it.)
        let mut after_cr = false;
        loop {
            let chunk = match reader.fill_buf() {
                Ok([]) => break,
                Ok(chunk) => chunk,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    // The receiver may be gone already; there is nobody left to tell.
                    let _ = tx.send((is_stderr, Err(e)));
                    return;
                }
            };
            let mut pieces = Vec::new();
            for (i, &byte) in chunk.iter().enumerate() {
                if std::mem::take(&mut after_cr) && byte == b'\n' {
                    continue;
                }
                match byte {
                    b'\n' => pieces.push(piece(&mut line, false)),
                    b'\r' => match chunk.get(i + 1) {
                        // The `\n` ends this line.
                        Some(b'\n') => {}
                        Some(_) => pieces.push(piece(&mut line, true)),
                        None => {
                            pieces.push(piece(&mut line, true));
                            after_cr = true;
                        }
                    },
                    _ => line.push(byte),
                }
            }
            let len = chunk.len();
            reader.consume(len);
            for p in pieces {
                if tx.send((is_stderr, Ok(p))).is_err() {
                    return;
                }
            }
        }
        if !line.is_empty() {
            // As above: a send fails only when the receiver is gone.
            let _ = tx.send((is_stderr, Ok(piece(&mut line, false))));
        }
    })
}

fn tool_name(program: &OsStr) -> String {
    Path::new(program)
        .file_name()
        .unwrap_or(program)
        .to_string_lossy()
        .into_owned()
}

fn spawn(program: &OsStr, cmd: &mut Command, opts: &RunOpts) -> Result<std::process::Child> {
    if let Some(cwd) = &opts.cwd {
        cmd.current_dir(cwd);
    }
    // Its own process group, so a kill reaches whatever the tool starts in turn.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(cmd, 0);
    cmd.spawn().map_err(|e| {
        Error::Io(std::io::Error::new(
            e.kind(),
            format!("cannot run {}: {e}", Path::new(program).display()),
        ))
    })
}

fn interrupted(opts: &RunOpts) -> bool {
    opts.interrupt
        .as_ref()
        .is_some_and(|f| f.load(Ordering::SeqCst))
}

/// Kills the child's whole process group, then reaps the child. Must run before the child
/// is reaped: until then its pid, which is also the group id, cannot be reused.
fn kill_and_reap(child: &mut std::process::Child) {
    #[cfg(unix)]
    kill_group(child);
    // Best effort: the child may have exited already, and a failed wait leaves nothing
    // more to do.
    let _ = child.kill();
    let _ = child.wait();
}

/// SIGKILL to the group that [`spawn`] made for `child`.
#[cfg(unix)]
fn kill_group(child: &std::process::Child) {
    let Ok(group) = libc::pid_t::try_from(child.id()) else {
        return;
    };
    // SAFETY: kill(2) only takes integers and touches no memory of this process. `-group`
    // names the child's own group (process_group(0) in `spawn`), never ours. The result is
    // ignored on purpose: ESRCH means the group is already gone, which is the goal.
    let _ = unsafe { libc::kill(-group, libc::SIGKILL) };
}

/// Forward every line from `rx` to `on_line` until all senders are gone, keeping the last
/// stderr lines. Of a run of `\r` updates the tail keeps only the latest, so progress does
/// not push the messages out. Kills the child on interrupt or on a read error.
fn drain(
    child: &mut std::process::Child,
    rx: &mpsc::Receiver<Line>,
    opts: &RunOpts,
    on_line: &mut dyn FnMut(bool, &str),
) -> Result<Vec<String>> {
    let mut tail: Vec<String> = Vec::new();
    let mut tail_ends_in_update = false;
    loop {
        if interrupted(opts) {
            // Readers are not joined: a grandchild may still hold the pipes open.
            kill_and_reap(child);
            return Err(Error::Interrupted);
        }
        match rx.recv_timeout(POLL_INTERVAL) {
            Ok((_, Err(e))) => {
                kill_and_reap(child);
                return Err(Error::Io(e));
            }
            Ok((is_stderr, Ok(Piece { text, update }))) => {
                on_line(is_stderr, &text);
                if is_stderr {
                    if update && tail_ends_in_update {
                        tail.pop();
                    } else if tail.len() == STDERR_TAIL_LINES {
                        tail.remove(0);
                    }
                    tail.push(text);
                    tail_ends_in_update = update;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Ok(tail),
        }
    }
}

/// The child may outlive its pipes, so keep honouring the interrupt flag while waiting.
fn wait_status(child: &mut std::process::Child, opts: &RunOpts) -> Result<ExitStatus> {
    wait_status_with(child, opts, std::process::Child::try_wait)
}

/// [`wait_status`] with the `try_wait` call injectable, so a test can make it fail.
fn wait_status_with(
    child: &mut std::process::Child,
    opts: &RunOpts,
    mut try_wait: impl FnMut(&mut std::process::Child) -> std::io::Result<Option<ExitStatus>>,
) -> Result<ExitStatus> {
    loop {
        match try_wait(child) {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(e) => {
                kill_and_reap(child);
                return Err(Error::Io(e));
            }
        }
        if interrupted(opts) {
            kill_and_reap(child);
            return Err(Error::Interrupted);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Run `argv[0]` with the remaining arguments (no shell), streaming each stdout and
/// stderr line to `on_line`. A non-zero exit is `Error::Tool` carrying the last stderr
/// lines.
pub fn run(argv: &[OsString], opts: RunOpts, on_line: &mut dyn FnMut(&str)) -> Result<Output> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| Error::Usage("empty command".into()))?;
    let tool = tool_name(program);
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn(program, &mut cmd, &opts)?;
    let (tx, rx) = mpsc::channel();
    let readers = [
        pump(
            child.stdout.take().expect("piped stdout"),
            false,
            tx.clone(),
        ),
        pump(child.stderr.take().expect("piped stderr"), true, tx),
    ];
    let tail = drain(&mut child, &rx, &opts, &mut |_, line| on_line(line))?;
    for reader in readers {
        let _ = reader.join();
    }
    let status = wait_status(&mut child, &opts)?;
    if status.success() {
        Ok(Output { status })
    } else {
        Err(Error::Tool {
            tool,
            code: status.code(),
            stderr: tail.join("\n"),
        })
    }
}

/// Like [`run`], but feeds `stdin` to the child and returns its stdout (lossy UTF-8)
/// instead of streaming it. Stderr lines still go to `on_stderr`. `stdin` is written from
/// a separate thread and then closed, so a large input cannot deadlock with the child's
/// output.
pub fn run_capture(
    argv: &[OsString],
    stdin: &[u8],
    opts: RunOpts,
    on_stderr: &mut dyn FnMut(&str),
) -> Result<String> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| Error::Usage("empty command".into()))?;
    let tool = tool_name(program);
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn(program, &mut cmd, &opts)?;
    let (tx, rx) = mpsc::channel();
    let mut child_stdin = child.stdin.take().expect("piped stdin");
    let input = stdin.to_vec();
    let writer = std::thread::spawn(move || {
        use std::io::Write;
        // Dropping `child_stdin` closes the pipe. A child that exits without reading all
        // of its input is not an error here: its exit status reports the failure.
        match child_stdin.write_all(&input) {
            Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => Err(e),
            _ => Ok(()),
        }
    });
    let mut stdout = child.stdout.take().expect("piped stdout");
    let out_tx = tx.clone();
    // Holding a sender until EOF keeps `drain` waiting for stdout as well as stderr.
    let out_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let res = stdout.read_to_end(&mut bytes).map(|_| bytes);
        drop(out_tx);
        res
    });
    let err_reader = pump(child.stderr.take().expect("piped stderr"), true, tx);

    let tail = drain(&mut child, &rx, &opts, &mut |_, line| on_stderr(line))?;
    let _ = err_reader.join();
    let bytes = match out_reader.join().expect("stdout reader panicked") {
        Ok(bytes) => bytes,
        Err(e) => {
            kill_and_reap(&mut child);
            return Err(Error::Io(e));
        }
    };
    let status = wait_status(&mut child, &opts)?;
    if !status.success() {
        return Err(Error::Tool {
            tool,
            code: status.code(),
            stderr: tail.join("\n"),
        });
    }
    writer.join().expect("stdin writer panicked")?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Configured tool locations. `None` means look the tool up on the search path.
/// The config layer maps its settings onto this.
#[derive(Debug, Default, Clone)]
pub struct ToolPaths {
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
    pub whisper: Option<PathBuf>,
    pub model: Option<PathBuf>,
    /// Directories to search, in `PATH` syntax; `None` uses the `PATH` environment variable.
    pub path: Option<OsString>,
}

/// A resolved executable and its version string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInfo {
    pub path: PathBuf,
    pub version: String,
}

/// What `doctor` reports: every tool found, and everything missing.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub ffmpeg: Option<ToolInfo>,
    pub ffprobe: Option<ToolInfo>,
    pub whisper: Option<ToolInfo>,
    pub model: Option<PathBuf>,
    pub missing: Vec<Missing>,
}

/// A fully resolved toolchain, as returned by [`preflight`].
#[derive(Debug, Clone)]
pub struct Toolchain {
    pub ffmpeg: ToolInfo,
    pub ffprobe: ToolInfo,
    pub whisper: ToolInfo,
    /// Present when the model was required.
    pub model: Option<PathBuf>,
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

fn find_in(name: &str, search: &OsStr) -> Option<PathBuf> {
    // Empty and relative entries mean "the current directory"; never run tools from there.
    std::env::split_paths(search)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|p| is_executable(p))
}

/// Run `<path> <flag>` and return the first non-empty output line, or why that failed.
fn probe_version(
    path: &Path,
    flag: &str,
    timeout: Duration,
) -> std::result::Result<String, String> {
    let done = Arc::new(AtomicBool::new(false));
    let timed_out = Arc::new(AtomicBool::new(false));
    let timer = {
        let (done, timed_out) = (Arc::clone(&done), Arc::clone(&timed_out));
        std::thread::spawn(move || {
            let deadline = Instant::now() + timeout;
            while !done.load(Ordering::SeqCst) {
                if Instant::now() >= deadline {
                    timed_out.store(true, Ordering::SeqCst);
                    return;
                }
                std::thread::sleep(POLL_INTERVAL);
            }
        })
    };
    let mut first = None;
    let argv = [path.as_os_str().to_owned(), OsString::from(flag)];
    let opts = RunOpts {
        interrupt: Some(Arc::clone(&timed_out)),
        ..RunOpts::default()
    };
    let res = run(&argv, opts, &mut |l| {
        if first.is_none() && !l.trim().is_empty() {
            first = Some(l.trim().to_string());
        }
    });
    done.store(true, Ordering::SeqCst);
    let _ = timer.join();
    match res {
        Ok(_) => first.ok_or_else(|| "it printed nothing".to_string()),
        Err(Error::Interrupted) => Err(format!("it timed out after {}s", timeout.as_secs_f32())),
        Err(e) => Err(e.to_string()),
    }
}

fn locate(
    configured: &Option<PathBuf>,
    name: &str,
    version_flag: &str,
    search: &OsStr,
    hint: &str,
    missing: &mut Vec<Missing>,
) -> Option<ToolInfo> {
    let found = match configured {
        Some(p) if is_executable(p) => Some(p.clone()),
        Some(p) => {
            missing.push(Missing {
                name: name.to_string(),
                hint: format!(
                    "configured path {} does not exist or is not executable",
                    p.display()
                ),
            });
            return None;
        }
        None => find_in(name, search),
    };
    let Some(path) = found else {
        missing.push(Missing {
            name: name.to_string(),
            hint: hint.to_string(),
        });
        return None;
    };
    let version = match probe_version(&path, version_flag, VERSION_PROBE_TIMEOUT) {
        Ok(v) => v,
        Err(why) => {
            missing.push(Missing {
                name: name.to_string(),
                hint: format!(
                    "found at {} but `{version_flag}` failed: {why}",
                    path.display()
                ),
            });
            "unknown".to_string()
        }
    };
    Some(ToolInfo { path, version })
}

/// Find a single tool by configured path (when `bin` has a directory part) or on `PATH`
/// and probe its version. `label` names it in the `Missing` entry.
pub fn locate_tool(
    bin: &str,
    label: &str,
    version_flag: &str,
    hint: &str,
) -> std::result::Result<ToolInfo, Missing> {
    let mut missing = Vec::new();
    let configured = if Path::new(bin).components().count() > 1 {
        Some(PathBuf::from(bin))
    } else {
        let search = std::env::var_os("PATH").unwrap_or_default();
        find_in(bin, &search)
    };
    let found = match configured {
        Some(p) => locate(
            &Some(p),
            label,
            version_flag,
            OsStr::new(""),
            hint,
            &mut missing,
        ),
        None => {
            missing.push(Missing {
                name: label.to_string(),
                hint: hint.to_string(),
            });
            None
        }
    };
    match (found, missing.pop()) {
        (Some(info), None) => Ok(info),
        (_, Some(m)) => Err(m),
        (None, None) => unreachable!("locate reports why it found nothing"),
    }
}

/// Look at every tool and report what was found and what is missing; never fails.
pub fn inspect(paths: &ToolPaths, need_model: bool) -> Report {
    let env_path = std::env::var_os("PATH").unwrap_or_default();
    let search = paths.path.as_deref().unwrap_or(&env_path);
    let mut missing = Vec::new();
    let ffmpeg = locate(
        &paths.ffmpeg,
        "ffmpeg",
        "-version",
        search,
        "brew install ffmpeg",
        &mut missing,
    );
    let ffprobe = locate(
        &paths.ffprobe,
        "ffprobe",
        "-version",
        search,
        "brew install ffmpeg (ffprobe ships with ffmpeg)",
        &mut missing,
    );
    let whisper = locate(
        &paths.whisper,
        "whisper-cli",
        "--version",
        search,
        "brew install whisper-cpp",
        &mut missing,
    );
    let model = paths.model.clone().filter(|p| p.is_file());
    // A configured model that is not there is always reported: it is most likely a typo.
    let hint = match (&paths.model, &model) {
        (Some(p), None) => Some(format!("configured path {} is not a file", p.display())),
        (None, _) if need_model => {
            Some("run `minuta models pull base` or set whisper.model".to_string())
        }
        _ => None,
    };
    if let Some(hint) = hint {
        missing.push(Missing {
            name: "model".to_string(),
            hint,
        });
    }
    Report {
        ffmpeg,
        ffprobe,
        whisper,
        model,
        missing,
    }
}

/// Resolve the toolchain, reporting every missing item at once in `Error::MissingTools`.
pub fn preflight(paths: &ToolPaths, need_model: bool) -> Result<Toolchain> {
    let report = inspect(paths, need_model);
    match (report.ffmpeg, report.ffprobe, report.whisper) {
        (Some(ffmpeg), Some(ffprobe), Some(whisper)) if report.missing.is_empty() => {
            Ok(Toolchain {
                ffmpeg,
                ffprobe,
                whisper,
                model: report.model,
            })
        }
        _ => Err(Error::MissingTools(report.missing)),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn hanging_version_probe_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hang");
        std::fs::write(&path, "#!/bin/sh\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let start = Instant::now();
        let err = probe_version(&path, "--version", Duration::from_millis(200)).unwrap_err();
        assert!(err.contains("timed out"), "err was {err:?}");
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn try_wait_error_kills_child() {
        use std::os::unix::process::ExitStatusExt;
        let program = OsStr::new("/bin/sleep");
        let mut cmd = Command::new(program);
        cmd.arg("30");
        let mut child = spawn(program, &mut cmd, &RunOpts::default()).unwrap();
        let err = wait_status_with(&mut child, &RunOpts::default(), |_| {
            Err(std::io::Error::other("injected"))
        })
        .unwrap_err();
        match err {
            Error::Io(e) => assert_eq!(e.to_string(), "injected"),
            other => panic!("expected Io error, got {other:?}"),
        }
        // Reaped: the cached status says SIGKILL.
        let status = child.try_wait().unwrap();
        assert_eq!(status.and_then(|s| s.signal()), Some(9));
    }
}
