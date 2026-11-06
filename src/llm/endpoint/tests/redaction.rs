use super::*;

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
