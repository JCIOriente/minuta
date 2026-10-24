//! External process runner and toolchain preflight. Tools are always run from an argv
//! array, never through a shell.

use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::Duration;

use crate::error::{Error, Missing, Result};

const STDERR_TAIL_LINES: usize = 20;
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

    let mut tail: Vec<String> = Vec::new();
    loop {
        if opts
            .interrupt
            .as_ref()
            .is_some_and(|f| f.load(Ordering::SeqCst))
        {
            // Readers are not joined: a grandchild may still hold the pipes open.
            let _ = child.kill();
            let _ = child.wait();
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
    let status = child.wait()?;
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
    std::env::split_paths(search)
        .map(|dir| dir.join(name))
        .find(|p| is_executable(p))
}

fn first_line_version(path: &Path, flag: &str) -> String {
    let mut first = None;
    let argv = [path.as_os_str().to_owned(), OsString::from(flag)];
    let ok = run(&argv, RunOpts::default(), &mut |l| {
        if first.is_none() && !l.trim().is_empty() {
            first = Some(l.trim().to_string());
        }
    })
    .is_ok();
    match first {
        Some(v) if ok => v,
        _ => "unknown".to_string(),
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
        Some(p) => is_executable(p).then(|| p.clone()),
        None => find_in(name, search),
    };
    match found {
        Some(path) => {
            let version = first_line_version(&path, version_flag);
            Some(ToolInfo { path, version })
        }
        None => {
            missing.push(Missing {
                name: name.to_string(),
                hint: hint.to_string(),
            });
            None
        }
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
    let model = match &paths.model {
        Some(p) if p.is_file() => Some(p.clone()),
        _ => None,
    };
    if need_model && model.is_none() {
        missing.push(Missing {
            name: "model".to_string(),
            hint: "run `minuta models pull base` or set whisper.model".to_string(),
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
