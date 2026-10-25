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
    schemas: Mutex<Vec<Option<serde_json::Value>>>,
}

impl ScriptedBackend {
    pub fn new(id: &str, context_tokens: usize, replies: Vec<Result<String>>) -> Self {
        ScriptedBackend {
            id: id.to_string(),
            context_tokens,
            replies: Mutex::new(replies.into()),
            calls: Mutex::new(Vec::new()),
            schemas: Mutex::new(Vec::new()),
        }
    }

    /// Every `(system, user)` pair received so far, in order.
    pub fn calls(&self) -> Vec<(String, String)> {
        self.calls.lock().expect("calls mutex poisoned").clone()
    }

    /// The schema of each call, in order: `None` for `complete`, `Some` for `complete_json`.
    pub fn schemas(&self) -> Vec<Option<serde_json::Value>> {
        self.schemas.lock().expect("schemas mutex poisoned").clone()
    }

    fn reply(
        &self,
        system: &str,
        user: &str,
        schema: Option<&serde_json::Value>,
    ) -> Result<String> {
        self.calls
            .lock()
            .expect("calls mutex poisoned")
            .push((system.to_string(), user.to_string()));
        self.schemas
            .lock()
            .expect("schemas mutex poisoned")
            .push(schema.cloned());
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

impl Backend for ScriptedBackend {
    fn id(&self) -> &str {
        &self.id
    }

    fn context_tokens(&self) -> usize {
        self.context_tokens
    }

    fn complete(&self, system: &str, user: &str, _interrupt: &AtomicBool) -> Result<String> {
        self.reply(system, user, None)
    }

    fn complete_json(
        &self,
        system: &str,
        user: &str,
        schema: &serde_json::Value,
        _interrupt: &AtomicBool,
    ) -> Result<String> {
        self.reply(system, user, Some(schema))
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
        assert!(matches!(
            b.complete("s3", "u3", &stop),
            Err(Error::Llm { transient: false, ref message, .. }) if message.contains("no replies left")
        ));
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
    fn scripted_backend_records_schema() {
        let b = ScriptedBackend::new("fake:x", 1000, vec![Ok("a".into()), Ok("b".into())]);
        let stop = AtomicBool::new(false);
        let schema = serde_json::json!({"type": "object"});
        b.complete_json("s1", "u1", &schema, &stop).unwrap();
        b.complete("s2", "u2", &stop).unwrap();
        assert_eq!(b.schemas(), vec![Some(schema), None]);
        assert_eq!(
            b.calls(),
            vec![
                ("s1".to_string(), "u1".to_string()),
                ("s2".to_string(), "u2".to_string()),
            ]
        );
    }

    #[test]
    fn scripted_backend_is_send_sync() {
        fn assert_ss<T: Send + Sync>() {}
        assert_ss::<ScriptedBackend>();
    }
}
