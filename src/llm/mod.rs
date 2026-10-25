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
    /// Like `complete`, but asks the model to answer with JSON matching `schema`.
    /// Backends without structured output ignore the schema.
    fn complete_json(
        &self,
        system: &str,
        user: &str,
        schema: &serde_json::Value,
        interrupt: &AtomicBool,
    ) -> Result<String> {
        let _ = schema;
        self.complete(system, user, interrupt)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendId {
    Ollama { model: String },
    Claude { model: Option<String> },
}

const VALID_FORMS: &str = "valid forms: ollama:<model>, claude, claude:<model>";

fn invalid(s: &str) -> Error {
    Error::Usage(format!("invalid LLM backend {s:?} ({VALID_FORMS})"))
}

/// Model names come from untrusted template frontmatter and are echoed in hints and URLs.
fn is_model_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '-' | '[' | ']' | '@')
}

impl FromStr for BackendId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        if s == "claude" {
            return Ok(BackendId::Claude { model: None });
        }
        match s.split_once(':') {
            Some((_, model)) if model.starts_with('-') || !model.chars().all(is_model_char) => {
                Err(Error::Usage(format!(
                    "invalid LLM backend {s:?}: model names may contain only letters, digits and `._:/-[]@`, and may not start with `-`"
                )))
            }
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
    fn complete_json_defaults_to_complete() {
        struct OnlyComplete;
        impl Backend for OnlyComplete {
            fn id(&self) -> &str {
                "fake:only"
            }
            fn context_tokens(&self) -> usize {
                1
            }
            fn complete(&self, system: &str, user: &str, _: &AtomicBool) -> Result<String> {
                Ok(format!("{system}|{user}"))
            }
        }
        let stop = AtomicBool::new(false);
        let schema = serde_json::json!({"type": "object"});
        assert_eq!(
            OnlyComplete
                .complete_json("s", "u", &schema, &stop)
                .unwrap(),
            "s|u"
        );
    }

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
    fn backend_id_accepts_namespaced_model() {
        assert_eq!(
            "ollama:library/gemma4:26b".parse::<BackendId>().unwrap(),
            BackendId::Ollama {
                model: "library/gemma4:26b".into()
            }
        );
    }

    #[test]
    fn backend_id_accepts_real_model_names() {
        for good in [
            "claude:sonnet[1m]",
            "claude:claude-sonnet-4-5@20250929",
            "ollama:hf.co/user/repo:Q4_K_M",
        ] {
            assert!(good.parse::<BackendId>().is_ok(), "{good}");
        }
    }

    #[test]
    fn backend_id_unsafe_name_error_names_allowed_chars_and_escapes() {
        let err = "ollama:a\u{7}b"
            .parse::<BackendId>()
            .unwrap_err()
            .to_string();
        assert!(err.contains("`._:/-[]@`"), "{err}");
        assert!(!err.contains('\u{7}'), "{err:?}");
    }

    #[test]
    fn backend_id_rejects_unsafe_model_names() {
        for bad in [
            "ollama:a b",
            "ollama:a;rm",
            "ollama:a\nb",
            "ollama:a\u{7}b",
            "claude:a b",
            "ollama:-rf",
            "claude:--flag",
        ] {
            let err = bad.parse::<BackendId>().unwrap_err();
            assert!(matches!(err, Error::Usage(_)), "{bad:?}: {err}");
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
