//! Finding the tools and probing their versions, for `doctor` and preflight.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::process::{run, RunOpts, POLL_INTERVAL};
use crate::error::{Error, Missing, Result};

/// How long a `--version` probe may take before the tool counts as broken.
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Configured tool locations. `None` means look the tool up on the search path.
/// The config layer maps its settings onto this.
#[derive(Debug, Default, Clone)]
pub(crate) struct ToolPaths {
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
pub(crate) struct Report {
    pub ffmpeg: Option<ToolInfo>,
    pub ffprobe: Option<ToolInfo>,
    pub whisper: Option<ToolInfo>,
    pub model: Option<PathBuf>,
    pub missing: Vec<Missing>,
}

/// A fully resolved toolchain, as returned by [`preflight`].
#[derive(Debug, Clone)]
pub(crate) struct Toolchain {
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
pub(crate) fn locate_tool(
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

const INSTALL_GUIDE: &str = "https://github.com/JCIOriente/minuta/blob/main/docs/install.md";

/// What to tell the user when `tool` is missing on `os` (a `std::env::consts::OS` value).
/// Homebrew installs all three tools on macOS and Linux; elsewhere the install guide is
/// the only place that covers the system.
pub(crate) fn install_hint(tool: &str, os: &str) -> String {
    match (os, tool) {
        ("macos" | "linux", "ffmpeg") => "brew install ffmpeg".to_string(),
        ("macos" | "linux", "ffprobe") => {
            "brew install ffmpeg (ffprobe ships with ffmpeg)".to_string()
        }
        ("macos" | "linux", "whisper-cli") => "brew install whisper.cpp".to_string(),
        _ => format!("see {INSTALL_GUIDE}"),
    }
}

/// Look at every tool and report what was found and what is missing; never fails.
pub(crate) fn inspect(paths: &ToolPaths, need_model: bool) -> Report {
    let env_path = std::env::var_os("PATH").unwrap_or_default();
    let search = paths.path.as_deref().unwrap_or(&env_path);
    let mut missing = Vec::new();
    let ffmpeg = locate(
        &paths.ffmpeg,
        "ffmpeg",
        "-version",
        search,
        &install_hint("ffmpeg", std::env::consts::OS),
        &mut missing,
    );
    let ffprobe = locate(
        &paths.ffprobe,
        "ffprobe",
        "-version",
        search,
        &install_hint("ffprobe", std::env::consts::OS),
        &mut missing,
    );
    let whisper = locate(
        &paths.whisper,
        "whisper-cli",
        "--version",
        search,
        &install_hint("whisper-cli", std::env::consts::OS),
        &mut missing,
    );
    // A configured model that is not there is always reported: it is most likely a typo.
    let (model, hint) = match &paths.model {
        Some(p) => match std::fs::metadata(p) {
            Ok(meta) if meta.is_file() => (Some(p.clone()), None),
            Ok(_) => (None, Some(not_a_file(p))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (None, Some(not_a_file(p))),
            Err(e) => (
                None,
                Some(format!("cannot read configured path {}: {e}", p.display())),
            ),
        },
        None if need_model => (
            None,
            Some("run `minuta models pull base` or set whisper.model".to_string()),
        ),
        None => (None, None),
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

fn not_a_file(path: &Path) -> String {
    format!("configured path {} is not a file", path.display())
}

/// Resolve the toolchain, reporting every missing item at once in `Error::MissingTools`.
pub(crate) fn preflight(paths: &ToolPaths, need_model: bool) -> Result<Toolchain> {
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
mod tests;
