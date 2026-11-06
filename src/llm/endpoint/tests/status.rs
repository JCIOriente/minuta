use super::*;
use crate::test_support::fake_http::{FakeHttp, Route};

fn status_err(status: u16, body: &'static str) -> (String, bool) {
    let fake = server(move |_| Reply::Status(status, body.to_string()));
    llm_err(complete(&fake).unwrap_err())
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
