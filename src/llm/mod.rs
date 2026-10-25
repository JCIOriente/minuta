//! LLM backends: the `Backend` trait and backend identifiers.

use std::fmt;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;

use crate::error::{Error, Result};

pub mod call;
pub mod chunk;
pub mod claude;
#[doc(hidden)] // Not cfg(test): integration tests in tests/ need it.
pub mod fake;
pub mod ollama;

pub const DEFAULT_LLM: &str = "ollama:gemma4:26b";

pub trait Backend: Send + Sync {
    /// Canonical id, e.g. "ollama:gemma4:26b".
    fn id(&self) -> &str;
    /// Prompt + output budget per call.
    fn context_tokens(&self) -> usize;
    fn complete(&self, system: &str, user: &str, interrupt: &AtomicBool) -> Result<String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendId {
    Ollama { model: String },
    Claude { model: Option<String> },
}

const VALID_FORMS: &str = "valid forms: ollama:<model>, claude, claude:<model>";

fn invalid(s: &str) -> Error {
    Error::Usage(format!("invalid LLM backend '{s}' ({VALID_FORMS})"))
}

impl FromStr for BackendId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        if s == "claude" {
            return Ok(BackendId::Claude { model: None });
        }
        match s.split_once(':') {
            Some(("ollama", model)) if !model.is_empty() => Ok(BackendId::Ollama {
                model: model.to_string(),
            }),
            Some(("claude", model)) if !model.is_empty() => Ok(BackendId::Claude {
                model: Some(model.to_string()),
            }),
            _ => Err(invalid(s)),
        }
    }
}

impl fmt::Display for BackendId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendId::Ollama { model } => write!(f, "ollama:{model}"),
            BackendId::Claude { model: None } => write!(f, "claude"),
            BackendId::Claude { model: Some(m) } => write!(f, "claude:{m}"),
        }
    }
}

impl BackendId {
    /// True when transcripts leave this machine.
    pub fn sends_off_machine(&self) -> bool {
        match self {
            BackendId::Ollama { .. } => false,
            BackendId::Claude { .. } => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_id_parses_ollama_with_tagged_model() {
        assert_eq!(
            "ollama:gemma4:26b".parse::<BackendId>().unwrap(),
            BackendId::Ollama {
                model: "gemma4:26b".into()
            }
        );
    }

    #[test]
    fn backend_id_parses_claude() {
        assert_eq!(
            "claude".parse::<BackendId>().unwrap(),
            BackendId::Claude { model: None }
        );
    }

    #[test]
    fn backend_id_parses_claude_model() {
        assert_eq!(
            "claude:sonnet".parse::<BackendId>().unwrap(),
            BackendId::Claude {
                model: Some("sonnet".into())
            }
        );
    }

    #[test]
    fn backend_id_rejects_invalid() {
        for bad in ["ollama:", "ollama", "claude:", "gpt:x", ""] {
            let err = bad.parse::<BackendId>().unwrap_err();
            assert!(
                matches!(err, Error::Usage(ref m) if m.contains("ollama:<model>")),
                "{bad:?}: {err}"
            );
        }
    }

    #[test]
    fn backend_id_display_round_trips() {
        for s in ["ollama:gemma4:26b", "claude", "claude:sonnet"] {
            assert_eq!(s.parse::<BackendId>().unwrap().to_string(), s);
        }
    }

    #[test]
    fn default_llm_parses() {
        assert!(DEFAULT_LLM.parse::<BackendId>().is_ok());
    }

    #[test]
    fn only_claude_sends_off_machine() {
        assert!(!"ollama:x".parse::<BackendId>().unwrap().sends_off_machine());
        assert!("claude".parse::<BackendId>().unwrap().sends_off_machine());
        assert!("claude:opus"
            .parse::<BackendId>()
            .unwrap()
            .sends_off_machine());
    }
}
