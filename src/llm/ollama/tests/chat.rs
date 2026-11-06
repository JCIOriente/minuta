//! `complete`: the request sent, context size, transport and interrupts.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::error::Error;
use crate::llm::ollama::Ollama;
use crate::llm::Backend;
use serde_json::{json, Value};

use crate::test_support::fake_http::{FakeHttp, Route};
use crate::test_support::fake_ollama::{FakeOllama, Reply, Responder};

use super::{info, llm_err, no_interrupt, ok_chat, show, BAD_PORT};

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
fn chat_sends_a_path_with_a_space_encoded() {
    let fake = FakeOllama::start(&["m"], show(None, false), ok_chat());
    let url = format!("{}/a b", fake.url());
    let o = Ollama::new(&url, "m", 1024, &info(None, false));
    let _ = o.complete("s", "u", &no_interrupt());
    assert_eq!(fake.requests()[0].path, "/a%20b/api/chat");
}

#[test]
fn chat_refuses_an_invalid_port() {
    let o = Ollama::new(BAD_PORT, "m", 1024, &info(None, false));
    let (message, transient) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert_eq!(message, "request failed: invalid port number");
    assert!(!transient);
}

#[test]
fn chat_refuses_a_proxy_scheme() {
    let fake = FakeOllama::start(&["m"], show(None, false), ok_chat());
    let url = fake.url().replacen("http://", "socks5://", 1);
    let o = Ollama::new(&url, "m", 1024, &info(None, false));
    let (message, transient) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert_eq!(message, "request failed: not an http or https URL");
    assert!(!transient);
    assert!(fake.requests().is_empty());
}

#[test]
fn chat_keeps_a_non_utf8_reply() {
    let server = FakeHttp::start(vec![Route::ok(
        "/templates/api/chat",
        b"{\"message\":{\"content\":\"a\xffb\"},\"done\":true}".to_vec(),
    )]);
    let o = Ollama::new(&server.base(), "m", 1024, &info(None, false));
    assert_eq!(o.complete("s", "u", &no_interrupt()).unwrap(), "a\u{fffd}b");
}
