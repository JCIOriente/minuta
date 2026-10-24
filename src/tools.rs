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

fn pump<R: Read + Send + 'static>(
    stream: R,
    is_stderr: bool,
    tx: mpsc::Sender<(bool, String)>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(|l| l.ok()) {
            if tx.send((is_stderr, line)).is_err() {
                break;
            }
        }
    })
}

/// Run `argv[0]` with the remaining arguments (no shell), streaming each stdout and
/// stderr line to `on_line`. A non-zero exit is `Error::Tool` carrying the last stderr
/// lines.
pub fn run(argv: &[OsString], opts: RunOpts, on_line: &mut dyn FnMut(&str)) -> Result<Output> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| Error::Usage("empty command".into()))?;
    let tool = Path::new(program)
        .file_name()
        .unwrap_or(program)
        .to_string_lossy()
        .into_owned();
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = &opts.cwd {
        cmd.current_dir(cwd);
    }
    let mut child = cmd.spawn()?;
    let (tx, rx) = mpsc::channel();
    let readers = [
        pump(
            child.stdout.take().expect("piped stdout"),
            false,
            tx.clone(),
        ),
        pump(child.stderr.take().expect("piped stderr"), true, tx),
    ];

    let interrupted = || {
        opts.interrupt
            .as_ref()
            .is_some_and(|f| f.load(Ordering::SeqCst))
    };
    let kill_and_reap = |child: &mut std::process::Child| {
        let _ = child.kill();
        let _ = child.wait();
    };

    let mut tail: Vec<String> = Vec::new();
    loop {
        if interrupted() {
            // Readers are not joined: a grandchild may still hold the pipes open.
            kill_and_reap(&mut child);
            return Err(Error::Interrupted);
        }
        match rx.recv_timeout(POLL_INTERVAL) {
            Ok((is_stderr, line)) => {
                on_line(&line);
                if is_stderr {
                    if tail.len() == STDERR_TAIL_LINES {
                        tail.remove(0);
                    }
                    tail.push(line);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    for reader in readers {
        let _ = reader.join();
    }
    // The child may outlive its pipes, so keep honouring the interrupt flag while waiting.
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if interrupted() {
            kill_and_reap(&mut child);
            return Err(Error::Interrupted);
        }
        std::thread::sleep(POLL_INTERVAL);
    };
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
    if need_model && model.is_none() {
        let hint = match &paths.model {
            Some(p) => format!("configured path {} is not a file", p.display()),
            None => "run `minuta models pull base` or set whisper.model".to_string(),
        };
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
}
