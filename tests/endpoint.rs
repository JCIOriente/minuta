use std::io::Write;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use minuta::config::{Config, Overrides};
use minuta::error::{Error, Missing};
use minuta::llm::endpoint::{check, connect, OpenAiCompatible, Problem};
use minuta::llm::Backend;
use serde_json::{json, Value};

#[path = "../src/test_support/fake_ollama.rs"]
mod fake_ollama;
use fake_ollama::{FakeOllama, Reply, Request};

#[path = "../src/test_support/fake_http.rs"]
mod fake_http;
use fake_http::{FakeHttp, Route};

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

fn complete(fake: &FakeOllama) -> minuta::error::Result<String> {
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

fn status_err(status: u16, body: &'static str) -> (String, bool) {
    let fake = server(move |_| Reply::Status(status, body.to_string()));
    llm_err(complete(&fake).unwrap_err())
}

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
fn endpoint_status_429_is_transient() {
    assert_eq!(
        status_err(429, r#"{"error":{"message":"slow down"}}"#),
        ("HTTP 429: slow down".to_string(), true)
    );
}

#[test]
fn endpoint_status_500_is_transient() {
    assert_eq!(
        status_err(500, r#"{"error":"boom"}"#),
        ("HTTP 500: boom".to_string(), true)
    );
}

#[test]
fn endpoint_status_503_is_transient() {
    assert_eq!(status_err(503, ""), ("HTTP 503".to_string(), true));
}

#[test]
fn endpoint_status_401_names_the_key_setting() {
    assert_eq!(
        status_err(401, r#"{"error":"bad key"}"#),
        (
            "HTTP 401: bad key; check the key in endpoints.box.api_key_env".to_string(),
            false
        )
    );
}

#[test]
fn endpoint_status_403_names_the_key_setting() {
    assert_eq!(
        status_err(403, ""),
        (
            "HTTP 403; check the key in endpoints.box.api_key_env".to_string(),
            false
        )
    );
}

#[test]
fn endpoint_status_404_names_the_url_setting() {
    assert_eq!(
        status_err(404, ""),
        (
            "HTTP 404; check endpoints.box.url, which usually ends in /v1".to_string(),
            false
        )
    );
}

#[test]
fn endpoint_status_400_is_not_transient() {
    assert_eq!(status_err(400, ""), ("HTTP 400".to_string(), false));
}

#[test]
fn endpoint_error_body_openai_shape() {
    assert_eq!(
        status_err(
            400,
            r#"{"error":{"message":"model 'x' not found","type":"t"}}"#
        )
        .0,
        "HTTP 400: model 'x' not found"
    );
}

#[test]
fn endpoint_error_body_lm_studio_shape() {
    assert_eq!(
        status_err(400, r#"{"error":"exceeds the available context size"}"#).0,
        "HTTP 400: exceeds the available context size"
    );
}

#[test]
fn endpoint_error_body_plain_text() {
    assert_eq!(status_err(400, "bad gateway").0, "HTTP 400: bad gateway");
}

#[test]
fn endpoint_error_body_drops_control_characters() {
    assert_eq!(
        status_err(400, "{\"error\":\"a\\u001b[2Jb\"}").0,
        "HTTP 400: a[2Jb"
    );
}

#[test]
fn endpoint_redirect_not_followed() {
    let fake = server(|request| {
        if request.path == CHAT {
            Reply::Redirect("/v1/elsewhere".into())
        } else {
            reply("followed")
        }
    });
    let (message, transient) = llm_err(complete(&fake).unwrap_err());
    assert_eq!(
        message,
        "HTTP 307; redirects are not followed, so set endpoints.box.url to the final address"
    );
    assert!(!transient);
    assert_eq!(fake.requests().len(), 1);
}

#[test]
fn endpoint_finish_length_is_not_transient() {
    for content in [json!("cut"), json!(""), Value::Null] {
        let fake = server(move |_| reply_with(content.clone(), "length"));
        assert_eq!(
            llm_err(complete(&fake).unwrap_err()),
            (
                "response cut off at the context limit; give the model a larger context on the \
                 server, or lower endpoints.box.context_tokens"
                    .to_string(),
                false
            )
        );
    }
}

#[test]
fn endpoint_finish_content_filter_is_not_transient() {
    let fake = server(|_| reply_with(json!("half a docu"), "content_filter"));
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response ended early (finish_reason content_filter)".to_string(),
            false
        )
    );
}

#[test]
fn endpoint_finish_error_is_transient() {
    let fake = server(|_| reply_with(json!("half a docu"), "error"));
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response ended early (finish_reason error)".to_string(),
            true
        )
    );
}

#[test]
fn endpoint_finish_unknown_reason_is_not_transient() {
    for reason in ["abort", "insufficient_system_resource"] {
        let fake = server(move |_| reply_with(json!("half a docu"), reason));
        assert_eq!(
            llm_err(complete(&fake).unwrap_err()),
            (
                format!("response ended early (finish_reason {reason})"),
                false
            )
        );
    }
}

#[test]
fn endpoint_finish_reason_with_control_characters_is_sanitized() {
    let fake = server(|_| reply_with(json!("half"), "a\u{1b}[2Jb"));
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()).0,
        "response ended early (finish_reason a[2Jb)"
    );
}

#[test]
fn endpoint_finish_normal_stops_are_accepted() {
    for reason in ["stop", "eos", "eos_token", "stop_sequence", "end_turn"] {
        let fake = server(move |_| reply_with(json!("fine"), reason));
        assert_eq!(complete(&fake).unwrap(), "fine", "{reason}");
    }
}

#[test]
fn endpoint_finish_error_shows_the_cause_on_the_choice() {
    let fake = server(|_| {
        let body = json!({"choices": [{
            "message": {"content": "half"},
            "finish_reason": "error",
            "error": {"message": "provider down"},
        }]});
        Reply::Status(200, body.to_string())
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response ended early (finish_reason error): provider down".to_string(),
            true
        )
    );
}

#[test]
fn endpoint_empty_content_shows_the_cause_on_the_choice() {
    let fake = server(|_| {
        let body = json!({"choices": [{"message": {"content": ""}, "error": "provider down"}]});
        Reply::Status(200, body.to_string())
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response has no message content: provider down".to_string(),
            true
        )
    );
}

fn reply_body(choice: Value) -> Reply {
    Reply::Status(200, json!({ "choices": [choice] }).to_string())
}

#[test]
fn endpoint_error_beside_text_without_a_reason_fails() {
    let fake = server(|_| {
        reply_body(json!({
            "message": {"content": "half a docu"},
            "finish_reason": null,
            "error": {"message": "provider down"},
        }))
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        ("response ended early: provider down".to_string(), true)
    );
}

#[test]
fn endpoint_error_beside_text_with_a_normal_stop_fails() {
    let fake = server(|_| {
        reply_body(json!({
            "message": {"content": "half a docu"},
            "finish_reason": "stop",
            "error": "provider down",
        }))
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response ended early (finish_reason stop): provider down".to_string(),
            true
        )
    );
}

#[test]
fn endpoint_top_level_error_beside_text_fails() {
    let fake = server(|_| {
        let body = json!({
            "choices": [{"message": {"content": "half a docu"}, "finish_reason": "stop"}],
            "error": {"message": "provider down"},
        });
        Reply::Status(200, body.to_string())
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()).0,
        "response ended early (finish_reason stop): provider down"
    );
}

#[test]
fn endpoint_finish_reason_that_is_not_a_string_is_unknown() {
    let fake = server(|_| reply_body(json!({"message": {"content": "half"}, "finish_reason": 3})));
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        ("response ended early (finish_reason 3)".to_string(), false)
    );
}

#[test]
fn endpoint_refusal_is_shown() {
    let fake = server(|_| {
        reply_body(json!({
            "message": {"content": null, "refusal": "I can't help with that"},
            "finish_reason": "stop",
        }))
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response has no message content: I can't help with that".to_string(),
            true
        )
    );
}

#[test]
fn endpoint_finish_content_filter_with_a_refusal_is_not_transient() {
    let fake = server(|_| {
        reply_body(json!({
            "message": {"content": null, "refusal": "no"},
            "finish_reason": "content_filter",
        }))
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response ended early (finish_reason content_filter): no".to_string(),
            false
        )
    );
}

#[test]
fn endpoint_refusal_beside_text_is_accepted() {
    let fake = server(|_| {
        reply_body(json!({
            "message": {"content": "whole document", "refusal": "no"},
            "finish_reason": "stop",
        }))
    });
    assert_eq!(complete(&fake).unwrap(), "whole document");
}

#[test]
fn endpoint_error_that_is_neither_object_nor_text_is_ignored() {
    for error in [json!(false), json!(0), json!([])] {
        let fake = server(move |_| {
            reply_body(json!({
                "message": {"content": "whole document"},
                "finish_reason": "stop",
                "error": error.clone(),
            }))
        });
        assert_eq!(complete(&fake).unwrap(), "whole document");
    }
}

#[test]
fn endpoint_error_without_a_message_beside_text_fails() {
    let fake = server(|_| {
        reply_body(json!({
            "message": {"content": "half a docu"},
            "finish_reason": "stop",
            "error": {"code": 502},
        }))
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response ended early (finish_reason stop)".to_string(),
            true
        )
    );
}

#[test]
fn endpoint_finish_without_a_reason_is_accepted() {
    let fake = server(|_| {
        let body = json!({"choices": [{"message": {"content": "fine"}}]});
        Reply::Status(200, body.to_string())
    });
    assert_eq!(complete(&fake).unwrap(), "fine");
}

#[test]
fn endpoint_error_in_a_200_reply_is_shown() {
    let fake = server(|_| {
        let body = json!({"error": {"code": 502, "message": "provider down"}});
        Reply::Status(200, body.to_string())
    });
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        (
            "response has no message content: provider down".to_string(),
            true
        )
    );
}

#[test]
fn endpoint_body_cut_short_is_transient() {
    let fake = server(|_| Reply::Raw("HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{".into()));
    let (message, transient) = llm_err(complete(&fake).unwrap_err());
    assert!(message.starts_with("unreadable response: "), "{message}");
    assert!(transient);
}

const KEY: &str = "sk-or-REAL";

fn keyed_err(status: u16, body: String) -> (String, bool) {
    let fake = server(move |_| Reply::Status(status, body.clone()));
    let url = format!("{}/v1", fake.url());
    let err = client_at(&url, Some(KEY))
        .complete("SYS", "USER", &no_interrupt())
        .unwrap_err();
    llm_err(err)
}

#[test]
fn endpoint_key_echoed_in_a_401_is_redacted() {
    let body = json!({"error": {"message": format!("Incorrect API key: {KEY}")}});
    assert_eq!(
        keyed_err(401, body.to_string()).0,
        "HTTP 401: Incorrect API key: [redacted]; check the key in endpoints.box.api_key_env"
    );
}

#[test]
fn endpoint_key_echoed_in_a_500_is_redacted() {
    assert_eq!(
        keyed_err(500, format!("upstream refused {KEY}")).0,
        "HTTP 500: upstream refused [redacted]"
    );
}

#[test]
fn endpoint_key_echoed_in_a_200_error_is_redacted() {
    let body = json!({"error": format!("refused {KEY}")});
    assert_eq!(
        keyed_err(200, body.to_string()).0,
        "response has no message content: refused [redacted]"
    );
}

#[test]
fn endpoint_key_at_the_excerpt_cut_is_redacted() {
    let (message, _) = keyed_err(500, format!("{}{KEY}", "x".repeat(195)));
    assert_eq!(message, format!("HTTP 500: {}[reda", "x".repeat(195)));
}

#[test]
fn endpoint_key_split_by_a_control_character_is_redacted() {
    let body = json!({"error": "refused sk-or\u{0}-REAL"});
    assert_eq!(
        keyed_err(400, body.to_string()).0,
        "HTTP 400: refused [redacted]"
    );
}

#[test]
fn endpoint_check_redacts_a_key_that_json_escapes() {
    let key = "a\"b";
    let fake = server(move |_| Reply::Status(200, json!({"auth": key}).to_string()));
    let url = format!("{}/v1", fake.url());
    assert_eq!(
        unreachable_hint(check("box", &url, Some(key), "m").unwrap_err()),
        format!(
            "start the server or fix endpoints.box.url; unexpected /models response \
             ({{\"auth\":\"[redacted]\"}}) at {url}"
        )
    );
}

#[test]
fn endpoint_bad_status_line_drops_control_characters() {
    let fake = server(|_| Reply::Raw("HTTP/1.1 \u{1b}c1 OK\r\n\r\n".into()));
    let (message, transient) = llm_err(complete(&fake).unwrap_err());
    assert!(!message.chars().any(char::is_control), "{message:?}");
    assert!(!transient);
}

#[test]
fn endpoint_error_body_cut_short_keeps_the_status() {
    let fake = server(|_| Reply::Raw("HTTP/1.1 500 X\r\nContent-Length: 100\r\n\r\n{".into()));
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        ("HTTP 500".to_string(), true)
    );
}

#[test]
fn endpoint_check_redacts_an_echoed_key_in_an_unexpected_reply() {
    let fake = server(|_| Reply::Status(200, json!({"auth": KEY}).to_string()));
    let url = format!("{}/v1", fake.url());
    assert_eq!(
        unreachable_hint(check("box", &url, Some(KEY), "m").unwrap_err()),
        format!(
            "start the server or fix endpoints.box.url; unexpected /models response \
             ({{\"auth\":\"[redacted]\"}}) at {url}"
        )
    );
}

#[test]
fn endpoint_check_redacts_an_echoed_key_in_listed_ids() {
    let fake = server(|_| model_list(&[KEY]));
    let url = format!("{}/v1", fake.url());
    assert_eq!(
        model_missing_hint(check("box", &url, Some(KEY), "m").unwrap_err()),
        "not offered by the server; it offers: [redacted]"
    );
}

#[test]
fn endpoint_check_drops_control_characters_from_a_bad_status_line() {
    let fake = server(|_| Reply::Raw("HTTP/1.1 \u{1b}c1 OK\r\n\r\n".into()));
    let url = format!("{}/v1", fake.url());
    let hint = unreachable_hint(check("box", &url, None, "m").unwrap_err());
    assert!(!hint.chars().any(char::is_control), "{hint:?}");
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
fn endpoint_empty_content_is_transient() {
    for content in [json!(""), json!("  \n"), Value::Null] {
        let fake = server(move |_| reply_with(content.clone(), "stop"));
        assert_eq!(
            llm_err(complete(&fake).unwrap_err()),
            ("response has no message content".to_string(), true)
        );
    }
}

#[test]
fn endpoint_reply_without_choices_is_transient() {
    let fake = server(|_| Reply::Status(200, "{}".into()));
    assert_eq!(
        llm_err(complete(&fake).unwrap_err()),
        ("response has no message content".to_string(), true)
    );
}

#[test]
fn endpoint_reply_that_is_not_json_is_not_transient() {
    let fake = server(|_| Reply::Status(200, "<html>".into()));
    let (message, transient) = llm_err(complete(&fake).unwrap_err());
    assert!(message.starts_with("unreadable response: "), "{message}");
    assert!(!transient);
}

#[test]
fn endpoint_refused_connection_is_transient() {
    let err = client_at(CLOSED, None)
        .complete("SYS", "USER", &no_interrupt())
        .unwrap_err();
    let (message, transient) = llm_err(err);
    assert!(message.starts_with("request failed: "), "{message}");
    assert!(transient);
}

#[test]
fn endpoint_check_sends_a_path_with_a_space_encoded() {
    let fake = server(|_| model_list(&["m"]));
    let url = format!("{}/a b", fake.url());
    let _ = check("box", &url, None, "m");
    assert_eq!(fake.requests()[0].path, "/a%20b/models");
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
fn endpoint_check_refuses_an_invalid_port() {
    let url = "http://127.0.0.1:99999";
    assert_eq!(
        unreachable_hint(check("box", url, None, "m").unwrap_err()),
        format!("start the server or fix endpoints.box.url; invalid port number at {url}")
    );
}

#[test]
fn endpoint_chat_refuses_an_invalid_port() {
    let url = "http://127.0.0.1:99999";
    let backend = OpenAiCompatible::new("box", "m", url, None, 1000, Duration::from_secs(5));
    let (message, transient) = llm_err(backend.complete("s", "u", &no_interrupt()).unwrap_err());
    assert_eq!(message, "request failed: invalid port number");
    assert!(!transient);
}

/// A listener that answers every connection with `reply`, one chunk every `pause`, and keeps the
/// connection open until the client closes it.
fn raw_server(reply: Vec<Vec<u8>>, pause: Duration) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            for (index, chunk) in reply.iter().enumerate() {
                if index > 0 {
                    thread::sleep(pause);
                }
                if stream.write_all(chunk).is_err() {
                    break;
                }
            }
            let _ = std::io::copy(&mut stream, &mut std::io::sink());
        }
    });
    url
}

#[test]
fn endpoint_tls_rejected_by_the_client_is_transient() {
    let url = raw_server(vec![b"HTTP/1.1 200 OK\r\n\r\n".to_vec()], Duration::ZERO);
    let url = url.replacen("http://", "https://", 1);
    let backend = OpenAiCompatible::new("box", "m", &url, None, 1000, Duration::from_secs(5));
    let (message, transient) = llm_err(backend.complete("s", "u", &no_interrupt()).unwrap_err());
    assert_eq!(
        message,
        format!(
            "request failed: {url}/chat/completions: io: received corrupt message of type \
             InvalidContentType"
        )
    );
    assert!(transient, "{message}");
}

/// The timeout bounds each read, not the body: a slow reply that keeps coming must finish.
#[test]
fn endpoint_read_timeout_restarts_with_each_read() {
    let body =
        json!({"choices": [{"message": {"content": "hi"}, "finish_reason": "stop"}]}).to_string();
    let mut reply = vec![format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes()];
    let pieces = body.len().div_ceil(15);
    reply.extend(body.as_bytes().chunks(pieces).map(<[u8]>::to_vec));
    let url = raw_server(reply, Duration::from_millis(200));
    let backend = OpenAiCompatible::new("box", "m", &url, None, 1000, Duration::from_secs(2));
    assert_eq!(backend.complete("s", "u", &no_interrupt()).unwrap(), "hi");
}

#[test]
fn endpoint_key_in_the_url_is_redacted_from_a_transport_error() {
    let key = "k3yk3yk3y";
    let url = format!("{}/{key}/v1", raw_server(vec![], Duration::ZERO));
    let url = url.replacen("http://", "https://", 1);
    let backend = OpenAiCompatible::new(
        "box",
        "m",
        &url,
        Some(key.into()),
        1000,
        Duration::from_millis(200),
    );
    let (message, _) = llm_err(backend.complete("s", "u", &no_interrupt()).unwrap_err());
    let shown = url.replace(key, "[redacted]");
    assert_eq!(
        message,
        format!("request failed: {shown}/chat/completions: timeout: per read")
    );
}

fn chat_reply(content: &str) -> String {
    format!(r#"{{"choices":[{{"message":{{"content":"{content}"}},"finish_reason":"stop"}}]}}"#)
}

/// A chat reply of exactly `size` bytes.
fn reply_of(size: usize) -> Vec<u8> {
    chat_reply(&"a".repeat(size - chat_reply("").len())).into_bytes()
}

const TEN_MIB: usize = 10 * 1024 * 1024;

#[test]
fn endpoint_reads_a_reply_of_ten_mib() {
    let body = reply_of(TEN_MIB);
    let server = FakeHttp::start(vec![Route::ok("/templates/chat/completions", body)]);
    let backend = OpenAiCompatible::new(
        "box",
        "m",
        &server.base(),
        None,
        1000,
        Duration::from_secs(5),
    );
    let reply = backend.complete("s", "u", &no_interrupt()).unwrap();
    assert_eq!(reply.len(), TEN_MIB - chat_reply("").len());
}

#[test]
fn endpoint_refuses_a_reply_over_ten_mib() {
    let body = reply_of(TEN_MIB + 1);
    let server = FakeHttp::start(vec![Route::ok("/templates/chat/completions", body)]);
    let backend = OpenAiCompatible::new(
        "box",
        "m",
        &server.base(),
        None,
        1000,
        Duration::from_secs(5),
    );
    let (message, transient) = llm_err(backend.complete("s", "u", &no_interrupt()).unwrap_err());
    assert_eq!(
        message,
        "unreadable response: the response body is larger than request limit: 10485761"
    );
    assert!(transient);
}

#[test]
fn endpoint_keeps_a_non_utf8_reply() {
    let server = FakeHttp::start(vec![Route::ok(
        "/templates/chat/completions",
        b"{\"choices\":[{\"message\":{\"content\":\"a\xffb\"},\"finish_reason\":\"stop\"}]}"
            .to_vec(),
    )]);
    let backend = OpenAiCompatible::new(
        "box",
        "m",
        &server.base(),
        None,
        1000,
        Duration::from_secs(5),
    );
    assert_eq!(
        backend.complete("s", "u", &no_interrupt()).unwrap(),
        "a\u{fffd}b"
    );
}

#[test]
fn endpoint_error_body_that_is_not_utf8_is_surfaced() {
    let server = FakeHttp::start(vec![Route::status(
        "/templates/chat/completions",
        502,
        b"bad\xffgateway".to_vec(),
    )]);
    let backend = OpenAiCompatible::new(
        "box",
        "m",
        &server.base(),
        None,
        1000,
        Duration::from_secs(5),
    );
    let (message, transient) = llm_err(backend.complete("s", "u", &no_interrupt()).unwrap_err());
    assert_eq!(message, "HTTP 502: bad\u{fffd}gateway");
    assert!(transient);
}

#[test]
fn endpoint_read_timeout_is_transient() {
    let fake = server(|_| Reply::Hang);
    let c = OpenAiCompatible::new(
        "box",
        "m",
        &format!("{}/v1", fake.url()),
        None,
        4096,
        Duration::from_millis(200),
    );
    let (message, transient) = llm_err(c.complete("SYS", "USER", &no_interrupt()).unwrap_err());
    assert_eq!(
        message,
        format!(
            "request failed: {}/v1/chat/completions: timeout: per read",
            fake.url()
        )
    );
    assert!(transient);
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
    call: impl FnOnce(&OpenAiCompatible, &AtomicBool) -> minuta::error::Result<String> + Send + 'static,
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

#[test]
fn endpoint_check_passes_for_a_listed_model() {
    let fake = server(|_| model_list(&["other", "m"]));
    let url = format!("{}/v1", fake.url());
    check("box", &url, None, "m").unwrap();
    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].path, MODELS);
    assert_eq!(requests[0].authorization, None);
}

#[test]
fn endpoint_check_sends_the_key() {
    let fake = server(|_| model_list(&["m"]));
    let url = format!("{}/v1", fake.url());
    check("box", &url, Some("k3y"), "m").unwrap();
    assert_eq!(
        fake.requests()[0].authorization.as_deref(),
        Some("Bearer k3y")
    );
}

#[test]
fn endpoint_check_needs_an_exact_match() {
    let fake = server(|_| model_list(&["m:latest", "mm"]));
    let url = format!("{}/v1", fake.url());
    assert_eq!(
        model_missing_hint(check("box", &url, None, "m").unwrap_err()),
        "not offered by the server; it offers: m:latest, mm"
    );
}

#[test]
fn endpoint_check_lists_at_most_ten_models() {
    let ids: Vec<String> = (0..12).map(|n| format!("m{n}")).collect();
    let fake = server(move |_| {
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        model_list(&ids)
    });
    let url = format!("{}/v1", fake.url());
    assert_eq!(
        model_missing_hint(check("box", &url, None, "m").unwrap_err()),
        "not offered by the server; it offers: m0, m1, m2, m3, m4, m5, m6, m7, m8, m9, and 2 more"
    );
}

#[test]
fn endpoint_check_with_no_models() {
    let fake = server(|_| model_list(&[]));
    let url = format!("{}/v1", fake.url());
    assert_eq!(
        model_missing_hint(check("box", &url, None, "m").unwrap_err()),
        "the server offers no models"
    );
}

#[test]
fn endpoint_check_drops_control_characters_from_listed_ids() {
    let fake = server(|_| model_list(&["a\u{1b}[2Jb"]));
    let url = format!("{}/v1", fake.url());
    assert_eq!(
        model_missing_hint(check("box", &url, None, "m").unwrap_err()),
        "not offered by the server; it offers: a[2Jb"
    );
}

#[test]
fn endpoint_check_refused_connection() {
    let hint = unreachable_hint(check("box", CLOSED, None, "m").unwrap_err());
    assert!(
        hint.starts_with("start the server or fix endpoints.box.url; "),
        "{hint}"
    );
    assert!(hint.ends_with(&format!(" at {CLOSED}")), "{hint}");
}

#[test]
fn endpoint_check_401_names_the_key_setting() {
    let fake = server(|_| Reply::Status(401, "{}".into()));
    let url = format!("{}/v1", fake.url());
    assert_eq!(
        unreachable_hint(check("box", &url, None, "m").unwrap_err()),
        format!("HTTP status 401 at {url}; check the key in endpoints.box.api_key_env")
    );
}

#[test]
fn endpoint_check_403_names_the_key_setting() {
    let fake = server(|_| Reply::Status(403, "{}".into()));
    let url = format!("{}/v1", fake.url());
    assert_eq!(
        unreachable_hint(check("box", &url, None, "m").unwrap_err()),
        format!("HTTP status 403 at {url}; check the key in endpoints.box.api_key_env")
    );
}

#[test]
fn endpoint_check_404_is_unreachable() {
    let fake = server(|_| Reply::Status(404, "{}".into()));
    let url = format!("{}/v1", fake.url());
    assert_eq!(
        unreachable_hint(check("box", &url, None, "m").unwrap_err()),
        format!("start the server or fix endpoints.box.url; HTTP status 404 at {url}")
    );
}

#[test]
fn endpoint_check_redirect_not_followed() {
    let fake = server(|request| {
        if request.path == MODELS {
            Reply::Redirect("/v1/elsewhere".into())
        } else {
            model_list(&["m"])
        }
    });
    let url = format!("{}/v1", fake.url());
    assert_eq!(
        unreachable_hint(check("box", &url, None, "m").unwrap_err()),
        format!(
            "start the server or fix endpoints.box.url; HTTP status 307 (redirects are not \
             followed) at {url}"
        )
    );
    assert_eq!(fake.requests().len(), 1);
}

#[test]
fn endpoint_check_unexpected_shape() {
    let fake = server(|_| Reply::Status(200, r#"{"models":[]}"#.into()));
    let url = format!("{}/v1", fake.url());
    assert_eq!(
        unreachable_hint(check("box", &url, None, "m").unwrap_err()),
        format!(
            "start the server or fix endpoints.box.url; unexpected /models response \
             ({{\"models\":[]}}) at {url}"
        )
    );
}

#[test]
fn endpoint_check_not_json() {
    let fake = server(|_| Reply::Status(200, "<html>".into()));
    let url = format!("{}/v1", fake.url());
    let hint = unreachable_hint(check("box", &url, None, "m").unwrap_err());
    assert!(hint.contains("unreadable /models response ("), "{hint}");
}
