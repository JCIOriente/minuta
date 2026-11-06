use std::sync::Arc;

use super::*;
use crate::test_support::fake_ollama::{FakeOllama, Reply, Request};

mod check;
mod redaction;
mod reply;
mod request;
mod status;
mod transport;

const CHAT: &str = "/v1/chat/completions";
const MODELS: &str = "/v1/models";
/// Nothing listens here: the discard port is never served.
const CLOSED: &str = "http://127.0.0.1:9/v1";

fn model_list(ids: &[&str]) -> Reply {
    let data: Vec<Value> = ids.iter().map(|id| json!({"id": id})).collect();
    Reply::Status(200, json!({"object": "list", "data": data}).to_string())
}

fn reply_with(content: Value, finish_reason: &str) -> Reply {
    let body = json!({"choices": [{
        "message": {
            "role": "assistant",
            "content": content,
            "reasoning": "secret thoughts",
            "reasoning_content": "secret thoughts",
        },
        "finish_reason": finish_reason,
    }]});
    Reply::Status(200, body.to_string())
}

fn reply(content: &str) -> Reply {
    reply_with(json!(content), "stop")
}

fn server(responder: impl Fn(&Request) -> Reply + Send + Sync + 'static) -> FakeOllama {
    FakeOllama::start_with_show(&[], None, Arc::new(responder))
}

fn client(fake: &FakeOllama) -> OpenAiCompatible {
    client_at(&format!("{}/v1", fake.url()), None)
}

fn client_at(url: &str, key: Option<&str>) -> OpenAiCompatible {
    OpenAiCompatible::new(
        "box",
        "m",
        url,
        key.map(str::to_string),
        4096,
        Duration::from_secs(30),
    )
}

fn no_interrupt() -> AtomicBool {
    AtomicBool::new(false)
}

fn complete(fake: &FakeOllama) -> Result<String> {
    client(fake).complete("SYS", "USER", &no_interrupt())
}

fn llm_err(e: Error) -> (String, bool) {
    match e {
        Error::Llm {
            backend,
            message,
            transient,
        } => {
            assert_eq!(backend, "box:m");
            (message, transient)
        }
        other => panic!("expected Error::Llm, got {other}"),
    }
}

fn unreachable_hint(problem: Problem) -> String {
    match problem {
        Problem::Unreachable(Missing { name, hint }) => {
            assert_eq!(name, "box");
            hint
        }
        other => panic!("expected Unreachable, got {other:?}"),
    }
}

fn model_missing_hint(problem: Problem) -> String {
    match problem {
        Problem::ModelMissing(Missing { name, hint }) => {
            assert_eq!(name, "box:m");
            hint
        }
        other => panic!("expected ModelMissing, got {other:?}"),
    }
}
