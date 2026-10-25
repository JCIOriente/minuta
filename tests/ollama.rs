use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use minuta::error::Error;
use minuta::llm::ollama::{check, Ollama, OllamaInfo};
use minuta::llm::Backend;
use serde_json::{json, Value};

#[path = "support/fake_ollama.rs"]
mod fake_ollama;
use fake_ollama::{FakeOllama, Reply, Responder};

fn show(context: Option<u64>, thinking: bool) -> Value {
    let mut info = json!({"general.architecture": "gemma4"});
    if let Some(n) = context {
        info["gemma4.context_length"] = json!(n);
    }
    let caps = if thinking {
        json!(["completion", "thinking"])
    } else {
        json!(["completion"])
    };
    json!({"model_info": info, "capabilities": caps})
}

fn ok_chat() -> Responder {
    Arc::new(|_| Reply::chat("hello"))
}

fn info(context_length: Option<usize>, thinking: bool) -> OllamaInfo {
    OllamaInfo {
        context_length,
        thinking,
    }
}

fn no_interrupt() -> AtomicBool {
    AtomicBool::new(false)
}

fn llm_err(e: Error) -> (String, bool) {
    match e {
        Error::Llm {
            message, transient, ..
        } => (message, transient),
        other => panic!("expected Error::Llm, got {other}"),
    }
}

#[test]
fn check_unreachable_is_missing() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let url = format!("http://127.0.0.1:{port}");
    let missing = check(&url, "gemma4:26b").unwrap_err();
    assert_eq!(missing.name, "ollama");
    assert!(
        missing.hint.starts_with("start Ollama (`ollama serve`)"),
        "{}",
        missing.hint
    );
}

#[test]
fn check_model_absent_hints_pull() {
    let fake = FakeOllama::start(&["other:1b"], show(None, false), ok_chat());
    let missing = check(&fake.url(), "gemma4:26b").unwrap_err();
    assert_eq!(missing.name, "ollama:gemma4:26b");
    assert_eq!(missing.hint, "ollama pull gemma4:26b");
}

#[test]
fn check_bare_name_matches_latest() {
    let fake = FakeOllama::start(&["gemma:latest"], show(Some(8192), false), ok_chat());
    assert!(check(&fake.url(), "gemma").is_ok());
}

#[test]
fn check_tagged_name_does_not_match_other_tag() {
    let fake = FakeOllama::start(&["gemma:2b"], show(None, false), ok_chat());
    assert!(check(&fake.url(), "gemma:7b").is_err());
}

#[test]
fn check_reads_context_and_thinking() {
    let fake = FakeOllama::start(&["m"], show(Some(262144), true), ok_chat());
    let got = check(&fake.url(), "m").unwrap();
    assert_eq!(got.context_length, Some(262144));
    assert!(got.thinking);
}

#[test]
fn check_without_context_or_thinking() {
    let fake = FakeOllama::start(&["m"], show(None, false), ok_chat());
    let got = check(&fake.url(), "m").unwrap();
    assert_eq!(got.context_length, None);
    assert!(!got.thinking);
}

#[test]
fn chat_request_shape() {
    let fake = FakeOllama::start(&["m"], show(None, false), ok_chat());
    let ollama = Ollama::new(&fake.url(), "m", 4096, &info(None, false));
    let out = ollama.complete("SYS", "USER", &no_interrupt()).unwrap();
    assert_eq!(out, "hello");
    let chats: Vec<_> = fake
        .requests()
        .into_iter()
        .filter(|r| r.path == "/api/chat")
        .collect();
    assert_eq!(chats.len(), 1);
    assert_eq!(chats[0].method, "POST");
    assert_eq!(
        chats[0].body,
        json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": "SYS"},
                {"role": "user", "content": "USER"},
            ],
            "stream": false,
            "options": {"num_ctx": 4096},
        })
    );
}

#[test]
fn think_false_only_for_thinking_models() {
    let fake = FakeOllama::start(&["m"], show(None, false), ok_chat());
    Ollama::new(&fake.url(), "m", 1024, &info(None, true))
        .complete("s", "u", &no_interrupt())
        .unwrap();
    Ollama::new(&fake.url(), "m", 1024, &info(None, false))
        .complete("s", "u", &no_interrupt())
        .unwrap();
    let bodies: Vec<Value> = fake.requests().into_iter().map(|r| r.body).collect();
    assert_eq!(bodies[0]["think"], json!(false));
    assert!(bodies[1].get("think").is_none());
}

#[test]
fn context_is_min_of_num_ctx_and_model() {
    let cases = [
        (16384, Some(8192), 8192),
        (4096, Some(262144), 4096),
        (4096, None, 4096),
    ];
    for (num_ctx, length, expected) in cases {
        let o = Ollama::new("http://127.0.0.1:1", "m", num_ctx, &info(length, false));
        assert_eq!(o.context_tokens(), expected, "{num_ctx} {length:?}");
    }
}

#[test]
fn id_is_canonical() {
    let o = Ollama::new("http://127.0.0.1:1", "gemma4:26b", 1, &info(None, false));
    assert_eq!(o.id(), "ollama:gemma4:26b");
}

#[test]
fn status_classification() {
    let cases = [
        (429, true),
        (500, true),
        (503, true),
        (400, false),
        (404, false),
    ];
    for (status, transient) in cases {
        let responder: Responder = Arc::new(move |_| Reply::error(status, "boom"));
        let fake = FakeOllama::start(&["m"], show(None, false), responder);
        let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false));
        let err = o.complete("s", "u", &no_interrupt()).unwrap_err();
        let (message, got) = llm_err(err);
        assert_eq!(got, transient, "status {status}: {message}");
    }
}

#[test]
fn not_found_hints_pull() {
    let responder: Responder = Arc::new(|_| Reply::error(404, "model 'm' not found"));
    let fake = FakeOllama::start(&["m"], show(None, false), responder);
    let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false));
    let (message, _) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert!(message.contains("ollama pull m"), "{message}");
}

#[test]
fn error_body_message_is_surfaced() {
    let responder: Responder = Arc::new(|_| Reply::error(400, "bad options"));
    let fake = FakeOllama::start(&["m"], show(None, false), responder);
    let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false));
    let (message, _) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert!(message.contains("bad options"), "{message}");
}

#[test]
fn connection_refused_is_transient() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let o = Ollama::new(
        &format!("http://127.0.0.1:{port}"),
        "m",
        1024,
        &info(None, false),
    );
    let (_, transient) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert!(transient);
}

#[test]
fn read_timeout_is_transient() {
    let responder: Responder = Arc::new(|_| Reply::Hang);
    let fake = FakeOllama::start(&["m"], show(None, false), responder);
    let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false))
        .with_timeout(Duration::from_millis(200));
    let (_, transient) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert!(transient);
}

#[test]
fn length_done_reason_is_error() {
    let responder: Responder = Arc::new(|_| Reply::chat_done("cut off", "length"));
    let fake = FakeOllama::start(&["m"], show(None, false), responder);
    let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false));
    let (message, transient) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert!(!transient);
    assert!(message.contains("raise ollama.num_ctx"), "{message}");
}

#[test]
fn malformed_response_is_non_transient_error() {
    let responder: Responder = Arc::new(|_| Reply::Status(200, "{\"nope\":1}".into()));
    let fake = FakeOllama::start(&["m"], show(None, false), responder);
    let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false));
    let (_, transient) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert!(!transient);
}

#[test]
fn interrupt_abandons_hanging_request() {
    let responder: Responder = Arc::new(|_| Reply::Hang);
    let fake = FakeOllama::start(&["m"], show(None, false), responder);
    let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false))
        .with_timeout(Duration::from_secs(5));
    let flag = Arc::new(AtomicBool::new(false));
    let setter = {
        let flag = flag.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            flag.store(true, Ordering::SeqCst);
        })
    };
    let start = Instant::now();
    let err = o.complete("s", "u", &flag).unwrap_err();
    assert!(matches!(err, Error::Interrupted), "{err}");
    assert!(start.elapsed() < Duration::from_millis(600));
    setter.join().unwrap();
}

#[test]
fn invalid_url_is_non_transient() {
    let o = Ollama::new("no-scheme", "m", 1024, &info(None, false));
    let (_, transient) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert!(!transient);
}

#[test]
fn non_json_ok_body_is_non_transient() {
    let responder: Responder = Arc::new(|_| Reply::Status(200, "not json".into()));
    let fake = FakeOllama::start(&["m"], show(None, false), responder);
    let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false));
    let (_, transient) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert!(!transient);
}

#[test]
fn plain_text_error_body_is_included() {
    let responder: Responder = Arc::new(|_| Reply::Status(400, "  plain proxy failure  ".into()));
    let fake = FakeOllama::start(&["m"], show(None, false), responder);
    let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false));
    let (message, _) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert!(message.contains("plain proxy failure"), "{message}");
}

#[test]
fn plain_text_404_body_is_included() {
    let responder: Responder = Arc::new(|_| Reply::Status(404, "page not found".into()));
    let fake = FakeOllama::start(&["m"], show(None, false), responder);
    let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false));
    let (message, _) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert!(message.contains("page not found"), "{message}");
}

#[test]
fn tags_without_models_array_is_server_problem() {
    let fake = FakeOllama::start_with_tags(json!({"unexpected": true}), ok_chat());
    let missing = check(&fake.url(), "m").unwrap_err();
    assert_eq!(missing.name, "ollama");
    assert!(
        missing.hint.contains("unexpected /api/tags response"),
        "{}",
        missing.hint
    );
}

#[test]
fn show_failure_after_tags_names_the_model() {
    let fake = FakeOllama::start_with_show(&["m"], None, ok_chat());
    let missing = check(&fake.url(), "m").unwrap_err();
    assert_eq!(missing.name, "ollama:m");
}
