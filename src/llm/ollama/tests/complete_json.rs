//! `complete_json`: structured output.

use std::sync::atomic::Ordering;
use std::thread;

use super::*;

fn schema() -> Value {
    json!({"type": "object", "properties": {"title": {"type": "string"}}, "required": ["title"]})
}

fn chat_bodies(fake: &FakeOllama) -> Vec<Value> {
    fake.requests()
        .into_iter()
        .filter(|r| r.path == "/api/chat")
        .map(|r| r.body)
        .collect()
}

#[test]
fn complete_json_sends_format() {
    let fake = FakeOllama::start(&["m"], show(None, false), ok_chat());
    let ollama = Ollama::new(&fake.url(), "m", 4096, &info(None, false));
    let out = ollama
        .complete_json("SYS", "USER", &schema(), &no_interrupt())
        .unwrap();
    assert_eq!(out, "hello");
    let bodies = chat_bodies(&fake);
    assert_eq!(bodies.len(), 1);
    assert_eq!(
        bodies[0],
        json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": "SYS"},
                {"role": "user", "content": "USER"},
            ],
            "stream": false,
            "options": {"num_ctx": 4096},
            "format": schema(),
        })
    );
}

#[test]
fn complete_sends_no_format() {
    let fake = FakeOllama::start(&["m"], show(None, false), ok_chat());
    let ollama = Ollama::new(&fake.url(), "m", 4096, &info(None, false));
    ollama.complete("s", "u", &no_interrupt()).unwrap();
    assert!(chat_bodies(&fake)[0].get("format").is_none());
}

#[test]
fn json_400_hints_ollama_version() {
    let responder: Responder = Arc::new(|_| Reply::error(400, "bad format"));
    let fake = FakeOllama::start(&["m"], show(None, false), responder);
    let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false));
    let err = o
        .complete_json("s", "u", &schema(), &no_interrupt())
        .unwrap_err();
    let (message, transient) = llm_err(err);
    assert!(!transient, "{message}");
    assert!(
        message.contains("structured output needs Ollama 0.5 or later"),
        "{message}"
    );
    assert!(message.contains("bad format"), "{message}");
}

#[test]
fn plain_400_has_no_version_hint() {
    let responder: Responder = Arc::new(|_| Reply::error(400, "bad format"));
    let fake = FakeOllama::start(&["m"], show(None, false), responder);
    let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false));
    let (message, _) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert_eq!(message, "HTTP 400: bad format");
}

#[test]
fn json_status_classification() {
    let cases = [(429, true), (503, true), (404, false)];
    for (status, transient) in cases {
        let responder: Responder = Arc::new(move |_| Reply::error(status, "boom"));
        let fake = FakeOllama::start(&["m"], show(None, false), responder);
        let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false));
        let err = o
            .complete_json("s", "u", &schema(), &no_interrupt())
            .unwrap_err();
        let (message, got) = llm_err(err);
        assert_eq!(got, transient, "status {status}: {message}");
    }
}

#[test]
fn json_think_false_for_thinking_models() {
    let fake = FakeOllama::start(&["m"], show(None, false), ok_chat());
    Ollama::new(&fake.url(), "m", 1024, &info(None, true))
        .complete_json("s", "u", &schema(), &no_interrupt())
        .unwrap();
    assert_eq!(chat_bodies(&fake)[0]["think"], json!(false));
}

#[test]
fn json_interrupt_returns_interrupted() {
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
    let err = o.complete_json("s", "u", &schema(), &flag).unwrap_err();
    assert!(matches!(err, Error::Interrupted), "{err}");
    setter.join().unwrap();
}
