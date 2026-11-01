//! `claude -p` backend: one subprocess per call, sandboxed (empty temporary working
//! directory, no tools, no session persistence, no settings files).

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Error, Missing, Result};
use crate::llm::subprocess::{run_interruptible, ScratchDir};
use crate::llm::Backend;
use crate::tools::{self, ToolInfo};

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
}

impl Claude {
    pub fn new(bin: PathBuf, model: Option<String>) -> Claude {
        let id = match &model {
            Some(m) => format!("claude:{m}"),
            None => "claude".to_string(),
        };
        Claude { bin, model, id }
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

    /// What a finished `claude` run means for the call. `claude` runs in its own process
    /// group, so the terminal's Ctrl-C never reaches it: a death from a signal is an
    /// interrupt only when the flag is set (the group kill raced its own exit). With the
    /// flag clear it crashed or was killed from outside, which a retry may get past.
    /// Mirrors `runner::interrupted_by`.
    fn reply(&self, result: Result<String>, interrupt: &AtomicBool) -> Result<String> {
        let out = match result {
            Ok(out) => out,
            Err(Error::Tool { code: None, .. }) if interrupt.load(Ordering::SeqCst) => {
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

impl Backend for Claude {
    fn id(&self) -> &str {
        &self.id
    }

    fn context_tokens(&self) -> usize {
        CONTEXT_TOKENS
    }

    fn complete(&self, system: &str, user: &str, interrupt: &AtomicBool) -> Result<String> {
        let scratch = ScratchDir::create("minuta-claude").map_err(|e| {
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
        self.reply(result, interrupt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn killed() -> Result<String> {
        Err(Error::Tool {
            tool: "claude".into(),
            code: None,
            stderr: String::new(),
        })
    }

    #[test]
    fn signal_death_with_flag_is_interrupted() {
        let claude = Claude::new(PathBuf::from("claude"), None);
        let err = claude.reply(killed(), &AtomicBool::new(true)).unwrap_err();
        assert!(matches!(err, Error::Interrupted), "{err:?}");
    }

    #[test]
    fn signal_death_without_interrupt_is_transient() {
        let claude = Claude::new(PathBuf::from("claude"), None);
        let err = claude.reply(killed(), &AtomicBool::new(false)).unwrap_err();
        assert!(
            matches!(&err, Error::Llm { backend, message, transient: true }
                if backend == "claude" && message == "exited with status none"),
            "{err:?}"
        );
    }
}
