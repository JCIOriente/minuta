//! `codex exec` backend: one subprocess per call, sandboxed (empty temporary working
//! directory, read-only sandbox, no session files, no user config).

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Error, Missing, Result};
use crate::llm::subprocess::{run_interruptible, ScratchDir};
use crate::llm::Backend;
use crate::tools::{self, ToolInfo};

const CONTEXT_TOKENS: usize = 128_000;
const INSTALL_HINT: &str = "install Codex: npm install -g @openai/codex";

/// Find the `codex` binary (configured path or `PATH`) and its version.
pub fn check(bin: &str) -> std::result::Result<ToolInfo, Missing> {
    tools::locate_tool(bin, "codex", "--version", INSTALL_HINT)
}

pub struct Codex {
    bin: PathBuf,
    model: Option<String>,
    id: String,
}

impl Codex {
    pub fn new(bin: PathBuf, model: Option<String>) -> Codex {
        let id = match &model {
            Some(m) => format!("codex:{m}"),
            None => "codex".to_string(),
        };
        Codex { bin, model, id }
    }

    /// Flags are confirmed against `codex exec --help` (0.166): `--ignore-user-config`
    /// keeps `~/.codex/config.toml` (tools, MCP servers, profiles) out of the run while
    /// auth still comes from `CODEX_HOME`; `--ephemeral` keeps session files off disk;
    /// the read-only sandbox runs in an empty directory. There is no system-prompt flag,
    /// so the prompt goes in as the `developer_instructions` config value, a TOML string.
    /// The user message is read from stdin (`-`). Option values use the `--flag=value`
    /// form so a value starting with `-` cannot be read as another flag.
    fn argv(&self, system: &str) -> Vec<OsString> {
        let mut argv: Vec<OsString> = vec![
            self.bin.clone().into_os_string(),
            "exec".into(),
            "--ephemeral".into(),
            "--skip-git-repo-check".into(),
            "--ignore-user-config".into(),
            "--ignore-rules".into(),
            "--sandbox=read-only".into(),
        ];
        if let Some(m) = &self.model {
            argv.push(format!("--model={m}").into());
        }
        argv.push(format!("--config=developer_instructions={}", toml_string(system)).into());
        argv.push("-".into());
        argv
    }

    /// What a finished `codex` run means for the call. Mirrors `Claude::reply`: `codex`
    /// runs in its own process group, so a death from a signal is an interrupt only when
    /// the flag is set; otherwise it crashed or was killed from outside, which a retry
    /// may get past.
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
                message: "codex returned no output".into(),
                transient: true,
            });
        }
        Ok(out)
    }
}

/// `s` as a TOML basic string, quotes included.
fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {
                write!(out, "\\u{:04X}", u32::from(c)).expect("writing to a String");
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Backend for Codex {
    fn id(&self) -> &str {
        &self.id
    }

    fn context_tokens(&self) -> usize {
        CONTEXT_TOKENS
    }

    fn complete(&self, system: &str, user: &str, interrupt: &AtomicBool) -> Result<String> {
        let scratch = ScratchDir::create("minuta-codex").map_err(|e| {
            Error::Io(std::io::Error::new(
                e.kind(),
                format!("cannot create a temporary directory for codex: {e}"),
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
            tool: "codex".into(),
            code: None,
            stderr: String::new(),
        })
    }

    #[test]
    fn signal_death_with_flag_is_interrupted() {
        let codex = Codex::new(PathBuf::from("codex"), None);
        let err = codex.reply(killed(), &AtomicBool::new(true)).unwrap_err();
        assert!(matches!(err, Error::Interrupted), "{err:?}");
    }

    #[test]
    fn signal_death_without_interrupt_is_transient() {
        let codex = Codex::new(PathBuf::from("codex"), None);
        let err = codex.reply(killed(), &AtomicBool::new(false)).unwrap_err();
        assert!(
            matches!(&err, Error::Llm { backend, message, transient: true }
                if backend == "codex" && message == "exited with status none"),
            "{err:?}"
        );
    }

    #[test]
    fn toml_string_escapes_what_toml_requires() {
        assert_eq!(toml_string("plain é"), "\"plain é\"");
        assert_eq!(
            toml_string("a\"b\\c\nd\te\rf\u{1}g\u{7f}"),
            "\"a\\\"b\\\\c\\nd\\te\\rf\\u0001g\\u007F\""
        );
    }
}
