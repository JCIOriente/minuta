//! `complete` failures: status classification and error messages.

use super::*;

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
    let o = Ollama::new(CLOSED, "m", 1024, &info(None, false));
    let (_, transient) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert!(transient);
}

#[test]
fn read_timeout_is_transient() {
    let responder: Responder = Arc::new(|_| Reply::Hang);
    let fake = FakeOllama::start(&["m"], show(None, false), responder);
    let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false))
        .with_timeout(Duration::from_millis(200));
    let (message, transient) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert_eq!(
        message,
        format!("request failed: {}/api/chat: timeout: per read", fake.url())
    );
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
fn error_body_that_is_not_utf8_is_surfaced() {
    let server = FakeHttp::start(vec![Route::status(
        "/templates/api/chat",
        400,
        b"bad\xffbody".to_vec(),
    )]);
    let o = Ollama::new(&server.base(), "m", 1024, &info(None, false));
    let (message, transient) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert_eq!(message, "HTTP 400: bad\u{fffd}body");
    assert!(!transient);
}

#[test]
fn chat_answered_with_a_307_is_not_transient() {
    let responder: Responder = Arc::new(|_| Reply::Redirect("/elsewhere".into()));
    let fake = FakeOllama::start(&["m"], show(None, false), responder);
    let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false));
    let (message, transient) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert_eq!(
        message,
        format!("request failed: {}/api/chat: redirect failed", fake.url())
    );
    assert!(!transient);
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
fn long_error_body_is_truncated() {
    let responder: Responder = Arc::new(|_| Reply::Status(400, "x".repeat(500)));
    let fake = FakeOllama::start(&["m"], show(None, false), responder);
    let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false));
    let (message, _) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert!(message.contains(&"x".repeat(200)), "{message}");
    assert!(!message.contains(&"x".repeat(201)), "{message}");
}

#[test]
fn long_json_error_field_is_truncated() {
    let responder: Responder = Arc::new(|_| Reply::error(500, &"x".repeat(500)));
    let fake = FakeOllama::start(&["m"], show(None, false), responder);
    let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false));
    let (message, _) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert!(message.contains(&"x".repeat(200)), "{message}");
    assert!(!message.contains(&"x".repeat(201)), "{message}");
}

#[test]
fn control_characters_are_stripped_from_error() {
    let responder: Responder = Arc::new(|_| Reply::error(400, "bad\u{1b}[31mred"));
    let fake = FakeOllama::start(&["m"], show(None, false), responder);
    let o = Ollama::new(&fake.url(), "m", 1024, &info(None, false));
    let (message, _) = llm_err(o.complete("s", "u", &no_interrupt()).unwrap_err());
    assert!(!message.contains('\u{1b}'), "{message:?}");
    assert!(message.contains("bad[31mred"), "{message:?}");
}
