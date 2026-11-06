use super::*;

#[test]
fn endpoint_check_drops_control_characters_from_a_bad_status_line() {
    let fake = server(|_| Reply::Raw("HTTP/1.1 \u{1b}c1 OK\r\n\r\n".into()));
    let url = format!("{}/v1", fake.url());
    let hint = unreachable_hint(check("box", &url, None, "m").unwrap_err());
    assert!(!hint.chars().any(char::is_control), "{hint:?}");
}

#[test]
fn endpoint_check_sends_a_path_with_a_space_encoded() {
    let fake = server(|_| model_list(&["m"]));
    let url = format!("{}/a b", fake.url());
    let _ = check("box", &url, None, "m");
    assert_eq!(fake.requests()[0].path, "/a%20b/models");
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
