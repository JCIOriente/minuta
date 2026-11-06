//! `check`: reachability, model lookup, `/api/tags` and `/api/show` parsing.

use super::*;

#[test]
fn check_unreachable_is_missing() {
    let missing = check(CLOSED, "gemma4:26b").unwrap_err();
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

/// `/api/tags` behind `hops` redirects, then a pulled model.
fn tags_behind_redirects(hops: usize) -> FakeHttp {
    let tags = |hop: usize| match hop {
        0 => "/templates/api/tags".to_string(),
        n => format!("/templates/hop{n}"),
    };
    let mut routes: Vec<Route> = (0..hops)
        .map(|hop| Route::redirect(&tags(hop), &tags(hop + 1)))
        .collect();
    let listed = json!({"models": [{"name": "m"}]}).to_string();
    routes.push(Route::ok(&tags(hops), listed));
    routes.push(Route::ok(
        "/templates/api/show",
        show(None, false).to_string(),
    ));
    FakeHttp::start(routes)
}

#[test]
fn check_follows_four_redirects() {
    let server = tags_behind_redirects(4);
    assert_eq!(check(&server.base(), "m"), Ok(info(None, false)));
}

#[test]
fn check_refuses_a_fifth_redirect() {
    let server = tags_behind_redirects(5);
    let base = server.base();
    assert_eq!(
        check(&base, "m").unwrap_err().hint,
        format!("start Ollama (`ollama serve`); too many redirects at {base}")
    );
}

#[test]
fn check_sends_a_path_with_a_space_encoded() {
    let server = FakeHttp::start(vec![
        Route::ok(
            "/a%20b/api/tags",
            json!({"models": [{"name": "m"}]}).to_string(),
        ),
        Route::ok("/a%20b/api/show", show(None, false).to_string()),
    ]);
    let url = server.base().replace("/templates", "/a b");
    assert_eq!(check(&url, "m"), Ok(info(None, false)));
    assert_eq!(
        server.requests(),
        vec!["/a%20b/api/tags", "/a%20b/api/show"]
    );
}

#[test]
fn check_refuses_an_invalid_port() {
    assert_eq!(
        check(BAD_PORT, "m").unwrap_err().hint,
        format!("start Ollama (`ollama serve`); invalid port number at {BAD_PORT}")
    );
}

#[test]
fn check_strips_control_characters_from_a_redirect_error() {
    let server = FakeHttp::start(vec![Route::redirect(
        "/templates/api/tags",
        "/caf\u{e9}\u{9b}x",
    )]);
    let base = server.base();
    assert_eq!(
        check(&base, "m").unwrap_err().hint,
        format!(
            "start Ollama (`ollama serve`); protocol: location header is malformed: /caf\u{e9}x at {base}"
        )
    );
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
    assert!(
        missing.hint.contains("ollama show m failed"),
        "{}",
        missing.hint
    );
}

#[test]
fn long_tags_body_is_truncated_in_hint() {
    let fake = FakeOllama::start_with_tags(json!({"unexpected": "y".repeat(500)}), ok_chat());
    let missing = check(&fake.url(), "m").unwrap_err();
    assert!(!missing.hint.contains(&"y".repeat(201)), "{}", missing.hint);
}
