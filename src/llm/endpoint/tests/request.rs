use std::sync::atomic::Ordering;
use std::thread;
use std::time::Instant;

use super::*;
use crate::config::{Config, Overrides};

fn schema() -> Value {
    json!({"type": "object", "properties": {"title": {"type": "string"}}, "required": ["title"]})
}

fn plain_body() -> Value {
    json!({
        "model": "m",
        "messages": [
            {"role": "system", "content": "SYS"},
            {"role": "user", "content": "USER"},
        ],
        "stream": false,
    })
}

fn structured_body() -> Value {
    let mut body = plain_body();
    body["response_format"] = json!({
        "type": "json_schema",
        "json_schema": {"name": "minuta", "strict": true, "schema": schema()},
    });
    body
}

fn bodies(fake: &FakeOllama) -> Vec<Value> {
    fake.requests().into_iter().map(|r| r.body).collect()
}

#[test]
fn endpoint_identity() {
    let c = client_at(CLOSED, None);
    assert_eq!(c.id(), "box:m");
    assert_eq!(c.context_tokens(), 4096);
}

#[test]
fn endpoint_sends_exact_body() {
    let fake = server(|_| reply("hello"));
    assert_eq!(complete(&fake).unwrap(), "hello");
    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, CHAT);
    assert_eq!(requests[0].body, plain_body());
}

#[test]
fn endpoint_trailing_slash_in_url_is_trimmed() {
    let fake = server(|_| reply("hello"));
    let url = format!("{}/v1/", fake.url());
    client_at(&url, None)
        .complete("SYS", "USER", &no_interrupt())
        .unwrap();
    assert_eq!(fake.requests()[0].path, CHAT);
}

#[test]
fn endpoint_structured_sends_response_format() {
    let fake = server(|_| reply("{}"));
    let out = client(&fake)
        .complete_json("SYS", "USER", &schema(), &no_interrupt())
        .unwrap();
    assert_eq!(out, "{}");
    assert_eq!(bodies(&fake), [structured_body()]);
}

fn refused_structured(status: u16) {
    let fake = server(move |request| {
        if request.body.get("response_format").is_some() {
            Reply::Status(
                status,
                r#"{"error":"response_format not supported"}"#.into(),
            )
        } else {
            reply("plain answer")
        }
    });
    let out = client(&fake)
        .complete_json("SYS", "USER", &schema(), &no_interrupt())
        .unwrap();
    assert_eq!(out, "plain answer");
    assert_eq!(bodies(&fake), [structured_body(), plain_body()]);
}

#[test]
fn endpoint_structured_400_resends_without_response_format() {
    refused_structured(400);
}

#[test]
fn endpoint_structured_422_resends_without_response_format() {
    refused_structured(422);
}

#[test]
fn endpoint_structured_resends_only_once() {
    let fake = server(|_| Reply::Status(400, r#"{"error":"too long"}"#.into()));
    let err = client(&fake)
        .complete_json("SYS", "USER", &schema(), &no_interrupt())
        .unwrap_err();
    assert_eq!(llm_err(err), ("HTTP 400: too long".to_string(), false));
    assert_eq!(bodies(&fake), [structured_body(), plain_body()]);
}

#[test]
fn endpoint_structured_500_does_not_resend() {
    let fake = server(|_| Reply::Status(500, "{}".into()));
    let err = client(&fake)
        .complete_json("SYS", "USER", &schema(), &no_interrupt())
        .unwrap_err();
    assert!(llm_err(err).1);
    assert_eq!(bodies(&fake), [structured_body()]);
}

#[test]
fn endpoint_plain_400_does_not_resend() {
    let fake = server(|_| Reply::Status(400, "{}".into()));
    complete(&fake).unwrap_err();
    assert_eq!(fake.requests().len(), 1);
}

#[test]
fn endpoint_config_reaches_the_backend() {
    let fake = server(|request| {
        if request.path == MODELS {
            model_list(&["m"])
        } else {
            Reply::Hang
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let text = format!(
        "[endpoints.box]\nurl = \"{}/v1\"\ncontext_tokens = 32768\ntimeout_secs = 1\n",
        fake.url()
    );
    std::fs::write(&path, text).unwrap();
    let config = Config::resolve(Overrides::default(), &|_| None, Some(&path)).unwrap();
    let backend = connect("box", &config.endpoints["box"], "m").unwrap();
    assert_eq!(backend.id(), "box:m");
    assert_eq!(backend.context_tokens(), 32768);
    let started = Instant::now();
    let (_, transient) = llm_err(
        backend
            .complete("SYS", "USER", &no_interrupt())
            .unwrap_err(),
    );
    assert!(transient);
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "the 1 s timeout was not applied"
    );
}

#[test]
fn endpoint_chat_sends_a_path_with_a_space_encoded() {
    let fake = server(|_| Reply::Status(500, "{}".into()));
    let url = format!("{}/a b", fake.url());
    let backend = OpenAiCompatible::new("box", "m", &url, None, 1000, Duration::from_secs(5));
    let _ = backend.complete("s", "u", &no_interrupt());
    assert_eq!(fake.requests()[0].path, "/a%20b/chat/completions");
}

#[test]
fn endpoint_bearer_sent_when_a_key_is_set() {
    let fake = server(|_| reply("hello"));
    let url = format!("{}/v1", fake.url());
    client_at(&url, Some("k3y"))
        .complete("SYS", "USER", &no_interrupt())
        .unwrap();
    assert_eq!(
        fake.requests()[0].authorization.as_deref(),
        Some("Bearer k3y")
    );
}

#[test]
fn endpoint_bearer_absent_without_a_key() {
    let fake = server(|_| reply("hello"));
    complete(&fake).unwrap();
    assert_eq!(fake.requests()[0].authorization, None);
}

fn interrupted(
    call: impl FnOnce(&OpenAiCompatible, &AtomicBool) -> Result<String> + Send + 'static,
) {
    let fake = server(|_| Reply::Hang);
    let c = client(&fake);
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let worker = thread::spawn(move || call(&c, &flag));
    let deadline = Instant::now() + Duration::from_secs(30);
    while fake.requests().is_empty() {
        assert!(Instant::now() < deadline, "the request never arrived");
        thread::yield_now();
    }
    stop.store(true, Ordering::SeqCst);
    let result = worker.join().unwrap();
    assert!(matches!(result, Err(Error::Interrupted)), "{result:?}");
}

#[test]
fn endpoint_interrupt_returns_interrupted() {
    interrupted(|c, flag| c.complete("SYS", "USER", flag));
}

#[test]
fn endpoint_structured_interrupt_returns_interrupted() {
    interrupted(|c, flag| c.complete_json("SYS", "USER", &schema(), flag));
}
