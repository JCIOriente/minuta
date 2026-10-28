//! `claude -p` backend: one subprocess per call, sandboxed (empty temporary working
//! directory, no tools, no session persistence, no settings files).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::error::{Error, Missing, Result};
use crate::llm::Backend;
use crate::tools::{self, RunOpts, ToolInfo};

const WATCH_INTERVAL: Duration = Duration::from_millis(10);
/// How long to wait for the Ctrl-C handler after `claude` died from a signal. Same value
/// as `SIGNAL_GRACE` in `src/runner.rs`; keep the two in step.
const SIGNAL_GRACE: Duration = Duration::from_millis(100);
const CONTEXT_TOKENS: usize = 150_000;
const INSTALL_HINT: &str = "install Claude Code: https://claude.com/claude-code";

/// Find the `claude` binary (configured path or `PATH`) and its version.
pub fn check(bin: &str) -> std::result::Result<ToolInfo, Missing> {
    tools::locate_tool(bin, "claude", "--version", INSTALL_HINT)
}

pub struct Claude {
    bin: PathBuf,
    model: Option<String>,
    id: String,
    signal_grace: Duration,
}

impl Claude {
    pub fn new(bin: PathBuf, model: Option<String>) -> Claude {
        let id = match &model {
            Some(m) => format!("claude:{m}"),
            None => "claude".to_string(),
        };
        Claude {
            bin,
            model,
            id,
            signal_grace: SIGNAL_GRACE,
        }
    }

    /// Replaces [`SIGNAL_GRACE`]. Tests use a long grace so that a delayed Ctrl-C never
    /// lands after it on a slow machine; the wait ends as soon as the flag is set.
    #[doc(hidden)]
    pub fn with_signal_grace(mut self, grace: Duration) -> Claude {
        self.signal_grace = grace;
        self
    }

    /// Flags are confirmed against `claude --help` (2.x): `--tools ""` disables every
    /// tool; `--setting-sources` documents only `user`, `project` and `local`, so the
    /// narrowest choice is `local`, which reads `.claude/settings.local.json` from the
    /// empty working directory (nothing). `--tools ""` only empties the built-in tool set,
    /// so `--strict-mcp-config` (with no `--mcp-config`) also stops user MCP servers and
    /// connectors from loading. Option values use the `--flag=value` form so a
    /// value starting with `-` cannot be read as another flag.
    fn argv(&self, system: &str) -> Vec<OsString> {
        let mut argv: Vec<OsString> = vec![
            self.bin.clone().into_os_string(),
            "-p".into(),
            "--output-format".into(),
            "text".into(),
            format!("--system-prompt={system}").into(),
            "--tools".into(),
            "".into(),
            "--no-session-persistence".into(),
            "--setting-sources=local".into(),
            "--strict-mcp-config".into(),
        ];
        if let Some(m) = &self.model {
            argv.push(format!("--model={m}").into());
        }
        argv
    }
}

/// A fresh, empty, private directory, removed on drop. Cleanup is best effort on every
/// path: a failure to remove it never discards a successful reply and never masks the
/// primary error.
struct ScratchDir(PathBuf);

impl ScratchDir {
    fn create() -> std::io::Result<ScratchDir> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let name = format!(
            "minuta-claude-{}-{nanos}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        let path = std::env::temp_dir().join(name);
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path)?;
        Ok(ScratchDir(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Sets the flag on drop, so the watcher stops even if the call panics.
struct SetOnDrop<'a>(&'a AtomicBool);

impl Drop for SetOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Whether `claude`, dead from a signal, was killed by Ctrl-C. The terminal signals the
/// whole process group, so `claude` can die before the handler has set the flag: poll it
/// for up to `grace`. Mirrors `runner::interrupted_by`.
fn signal_was_interrupt(interrupt: &AtomicBool, grace: Duration) -> bool {
    let deadline = Instant::now() + grace;
    loop {
        if interrupt.load(Ordering::SeqCst) {
            return true;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        std::thread::sleep(left.min(WATCH_INTERVAL));
    }
}

/// `tools` wants an owned flag, the trait hands over a borrowed one: a watcher thread
/// mirrors the caller's flag for the duration of the call.
fn run_interruptible(
    argv: &[OsString],
    stdin: &[u8],
    cwd: &Path,
    interrupt: &AtomicBool,
) -> Result<String> {
    let mirror = Arc::new(AtomicBool::new(false));
    let done = AtomicBool::new(false);
    let opts = RunOpts {
        cwd: Some(cwd.to_path_buf()),
        interrupt: Some(Arc::clone(&mirror)),
    };
    std::thread::scope(|scope| {
        scope.spawn(|| {
            while !done.load(Ordering::SeqCst) {
                if interrupt.load(Ordering::SeqCst) {
                    mirror.store(true, Ordering::SeqCst);
                    return;
                }
                std::thread::sleep(WATCH_INTERVAL);
            }
        });
        let _stop = SetOnDrop(&done);
        tools::run_capture(argv, stdin, opts, &mut |_| {})
    })
}

impl Backend for Claude {
    fn id(&self) -> &str {
        &self.id
    }

    fn context_tokens(&self) -> usize {
        CONTEXT_TOKENS
    }

    fn complete(&self, system: &str, user: &str, interrupt: &AtomicBool) -> Result<String> {
        let scratch = ScratchDir::create().map_err(|e| {
            Error::Io(std::io::Error::new(
                e.kind(),
                format!("cannot create a temporary directory for claude: {e}"),
            ))
        })?;
        let result = run_interruptible(
            &self.argv(system),
            user.as_bytes(),
            scratch.path(),
            interrupt,
        );
        drop(scratch);
        let out = match result {
            Ok(out) => out,
            Err(Error::Tool { code: None, .. })
                if signal_was_interrupt(interrupt, self.signal_grace) =>
            {
                return Err(Error::Interrupted);
            }
            Err(Error::Tool { code, stderr, .. }) => {
                let message = if stderr.trim().is_empty() {
                    format!(
                        "exited with status {}",
                        code.map_or("none".into(), |c| c.to_string())
                    )
                } else {
                    stderr
                };
                return Err(Error::Llm {
                    backend: self.id.clone(),
                    message,
                    transient: true,
                });
            }
            Err(e) => return Err(e),
        };
        if out.trim().is_empty() {
            return Err(Error::Llm {
                backend: self.id.clone(),
                message: "claude returned no output".into(),
                transient: true,
            });
        }
        Ok(out)
    }
}
