//! Running a tool: spawn in its own process group, stream its output, keep a stderr tail.

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

use super::progress_line::progress_line;
use crate::error::{Error, Result};

const STDERR_TAIL_LINES: usize = 20;
pub(super) const POLL_INTERVAL: Duration = Duration::from_millis(25);

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
pub struct Output;

/// One line of a tool's output.
struct Piece {
    text: String,
    /// It ended in a bare `\r`: a progress update (ffmpeg's stats) that the next one
    /// overwrites on a terminal.
    update: bool,
}

/// What a reader thread sends.
enum Out {
    Piece(Piece),
    /// The last piece was sent as an update, but its `\r` turned out to be the first half
    /// of a `\r\n` split across two reads: it is a full line after all.
    LastWasLine,
}

type Line = (bool, std::io::Result<Out>);

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
        // next chunk belongs to it.
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
            let send = |p: Piece| tx.send((is_stderr, Ok(Out::Piece(p)))).is_ok();
            for (i, &byte) in chunk.iter().enumerate() {
                let continues_cr = std::mem::take(&mut after_cr);
                let sent = match byte {
                    b'\n' if continues_cr => tx.send((is_stderr, Ok(Out::LastWasLine))).is_ok(),
                    b'\n' => send(piece(&mut line, false)),
                    b'\r' => match chunk.get(i + 1) {
                        // The `\n` ends this line.
                        Some(b'\n') => true,
                        Some(_) => send(piece(&mut line, true)),
                        None => {
                            after_cr = true;
                            send(piece(&mut line, true))
                        }
                    },
                    _ => {
                        line.push(byte);
                        true
                    }
                };
                if !sent {
                    return;
                }
            }
            let len = chunk.len();
            reader.consume(len);
        }
        if !line.is_empty() {
            // As above: a send fails only when the receiver is gone.
            let _ = tx.send((is_stderr, Ok(Out::Piece(piece(&mut line, false)))));
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
    // more to do. On unix this only matters when the group kill could not be sent.
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
/// stderr lines in a [`Tail`]. Kills the child on interrupt or on a read error.
fn drain(
    child: &mut std::process::Child,
    rx: &mpsc::Receiver<Line>,
    opts: &RunOpts,
    on_line: &mut dyn FnMut(bool, &str),
) -> Result<Vec<String>> {
    let mut tail = Tail::default();
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
            Ok((is_stderr, Ok(Out::Piece(Piece { text, update })))) => {
                on_line(is_stderr, &text);
                if is_stderr {
                    tail.push(text, update);
                }
            }
            Ok((is_stderr, Ok(Out::LastWasLine))) => {
                if is_stderr {
                    tail.settle();
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Ok(tail.lines),
        }
    }
}

/// The last [`STDERR_TAIL_LINES`] stderr lines, so progress does not push the messages
/// out: progress lines ([`progress_line`]) are left out, whether they end in `\r` (ffmpeg)
/// or `\n` (whisper-cli), and of a run of other `\r` updates only the latest is kept.
#[derive(Debug, Default)]
struct Tail {
    lines: Vec<String>,
    ends_in_update: bool,
}

impl Tail {
    fn push(&mut self, text: String, update: bool) {
        if progress_line(&text).is_some() {
            return;
        }
        if update && self.ends_in_update {
            self.lines.pop();
        } else if self.lines.len() == STDERR_TAIL_LINES {
            self.lines.remove(0);
        }
        self.lines.push(text);
        self.ends_in_update = update;
    }

    /// The last line was a full line, not an update: the next update must not replace it.
    fn settle(&mut self) {
        self.ends_in_update = false;
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
        Ok(Output)
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

#[cfg(all(test, unix))]
mod tests;
