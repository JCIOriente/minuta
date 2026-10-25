//! Scripted in-memory backend for tests.

use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use super::Backend;
use crate::error::{Error, Result};

/// Returns scripted replies in order and records each `(system, user)` it receives.
pub struct ScriptedBackend {
    id: String,
    context_tokens: usize,
    replies: Mutex<VecDeque<Result<String>>>,
    calls: Mutex<Vec<(String, String)>>,
}

impl ScriptedBackend {
    pub fn new(id: &str, context_tokens: usize, replies: Vec<Result<String>>) -> Self {
        ScriptedBackend {
            id: id.to_string(),
            context_tokens,
            replies: Mutex::new(replies.into()),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Every `(system, user)` pair received so far, in order.
    pub fn calls(&self) -> Vec<(String, String)> {
        self.calls.lock().expect("calls mutex poisoned").clone()
    }
}

impl Backend for ScriptedBackend {
    fn id(&self) -> &str {
        &self.id
    }

    fn context_tokens(&self) -> usize {
        self.context_tokens
    }

    fn complete(&self, system: &str, user: &str, _interrupt: &AtomicBool) -> Result<String> {
        self.calls
            .lock()
            .expect("calls mutex poisoned")
            .push((system.to_string(), user.to_string()));
        self.replies
            .lock()
            .expect("replies mutex poisoned")
            .pop_front()
            .unwrap_or_else(|| {
                Err(Error::Llm {
                    backend: self.id.clone(),
                    message: "scripted backend has no replies left".into(),
                    transient: false,
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_backend_returns_replies_in_order_and_records_calls() {
        let b = ScriptedBackend::new(
            "fake:x",
            1000,
            vec![
                Ok("one".into()),
                Err(Error::Llm {
                    backend: "fake:x".into(),
                    message: "boom".into(),
                    transient: true,
                }),
            ],
        );
        let stop = AtomicBool::new(false);
        assert_eq!(b.id(), "fake:x");
        assert_eq!(b.context_tokens(), 1000);
        assert_eq!(b.complete("s1", "u1", &stop).unwrap(), "one");
        assert!(matches!(
            b.complete("s2", "u2", &stop),
            Err(Error::Llm {
                transient: true,
                ..
            })
        ));
        assert!(b.complete("s3", "u3", &stop).is_err());
        assert_eq!(
            b.calls(),
            vec![
                ("s1".to_string(), "u1".to_string()),
                ("s2".to_string(), "u2".to_string()),
                ("s3".to_string(), "u3".to_string()),
            ]
        );
    }

    #[test]
    fn scripted_backend_is_send_sync() {
        fn assert_ss<T: Send + Sync>() {}
        assert_ss::<ScriptedBackend>();
    }
}
