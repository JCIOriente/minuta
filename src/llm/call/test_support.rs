use super::*;
use crate::llm::fake::ScriptedBackend;

pub(super) fn policy(retries: u32) -> Policy {
    Policy {
        retries,
        backoff: Duration::ZERO,
    }
}

pub(super) fn backend(replies: Vec<Result<String>>) -> ScriptedBackend {
    ScriptedBackend::new("fake:x", 1000, replies)
}

pub(super) fn run(
    b: &ScriptedBackend,
    cache: &CallCache,
    p: &Policy,
    refresh: bool,
) -> Result<String> {
    call(b, cache, p, "sys", "usr", refresh, &AtomicBool::new(false))
}

pub(super) fn schema() -> serde_json::Value {
    serde_json::json!({"type": "object", "required": ["title"]})
}
