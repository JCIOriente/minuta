use super::*;

#[test]
fn fetch_catalog_parses() {
    let a = entry("alpha", &template_text("a"));
    let b = entry("beta", &template_text("b"));
    let server = FakeHttp::start(vec![Route::ok(
        "/templates/index.toml",
        catalog_text(&[&a, &b]),
    )]);
    let catalog = fetch_catalog(&Origin::new(&server.base()).unwrap()).unwrap();
    assert_eq!(catalog.entries, vec![a, b]);
    assert_eq!(server.requests(), vec!["/templates/index.toml"]);
}

#[test]
fn fetch_catalog_http_error_names_the_url() {
    let server = FakeHttp::start(vec![]);
    let base = server.base();
    let text = err_text(fetch_catalog(&Origin::new(&base).unwrap()));
    assert_eq!(
        text,
        format!("I/O error: download of {base}/index.toml failed: HTTP status 404")
    );
}

#[test]
fn fetch_catalog_invalid_toml_is_a_parse_error() {
    let server = FakeHttp::start(vec![Route::ok("/templates/index.toml", "version = [")]);
    match fetch_catalog(&Origin::new(&server.base()).unwrap()) {
        Err(Error::Parse(m)) => assert!(m.starts_with("template catalog: "), "{m}"),
        other => panic!("expected parse error, got {other:?}"),
    }
}

#[test]
fn fetch_catalog_not_utf8_is_a_parse_error() {
    let server = FakeHttp::start(vec![Route::ok(
        "/templates/index.toml",
        vec![0xff, 0xfe, 0xfd],
    )]);
    match fetch_catalog(&Origin::new(&server.base()).unwrap()) {
        Err(Error::Parse(m)) => assert_eq!(m, "template catalog: the file is not valid UTF-8"),
        other => panic!("expected parse error, got {other:?}"),
    }
}

fn padded_catalog(len: u64) -> String {
    let mut s = String::from("version = 1\n");
    while (s.len() as u64) + 2 <= len {
        s.push_str("#\n");
    }
    while (s.len() as u64) < len {
        s.push('#');
    }
    assert_eq!(s.len() as u64, len);
    s
}

#[test]
fn fetch_catalog_accepts_a_body_of_exactly_the_cap() {
    let server = FakeHttp::start(vec![Route::ok(
        "/templates/index.toml",
        padded_catalog(CATALOG_CAP),
    )]);
    let catalog = fetch_catalog(&Origin::new(&server.base()).unwrap()).unwrap();
    assert_eq!(catalog.entries, vec![]);
}

#[test]
fn fetch_catalog_rejects_a_body_over_the_cap() {
    let server = FakeHttp::start(vec![Route::ok(
        "/templates/index.toml",
        padded_catalog(CATALOG_CAP + 1),
    )]);
    let base = server.base();
    assert_eq!(
        err_text(fetch_catalog(&Origin::new(&base).unwrap())),
        format!("I/O error: response from {base}/index.toml is larger than 1048576 bytes")
    );
}

#[test]
fn fetch_file_returns_the_bytes() {
    let text = template_text("a");
    let a = entry("alpha", &text);
    let server = FakeHttp::start(vec![Route::ok("/templates/alpha.md", text.clone())]);
    let bytes = fetch_file(&Origin::new(&server.base()).unwrap(), &a).unwrap();
    assert_eq!(bytes, text.as_bytes());
    assert_eq!(server.requests(), vec!["/templates/alpha.md"]);
}

#[test]
fn fetch_file_accepts_a_body_of_exactly_the_cap() {
    let a = entry("alpha", "x");
    let body = vec![b'a'; FILE_CAP as usize];
    let server = FakeHttp::start(vec![Route::ok("/templates/alpha.md", body.clone())]);
    assert_eq!(
        fetch_file(&Origin::new(&server.base()).unwrap(), &a).unwrap(),
        body
    );
}

#[test]
fn fetch_file_rejects_a_body_over_the_cap() {
    let a = entry("alpha", "x");
    let server = FakeHttp::start(vec![Route::ok(
        "/templates/alpha.md",
        vec![b'a'; FILE_CAP as usize + 1],
    )]);
    let base = server.base();
    assert_eq!(
        err_text(fetch_file(&Origin::new(&base).unwrap(), &a)),
        format!("I/O error: response from {base}/alpha.md is larger than 262144 bytes")
    );
}

#[test]
fn fetch_file_rejects_a_name_the_catalog_would_not_have_allowed() {
    let mut a = entry("alpha", "x");
    a.name = "../x".to_string();
    let origin = Origin::new("https://h.example/t").unwrap();
    match fetch_file(&origin, &a) {
        Err(Error::Usage(m)) => assert_eq!(
            m,
            "invalid template name \"../x\" (use lowercase letters, digits, '-' and '_', starting with a letter or digit)"
        ),
        other => panic!("expected usage error, got {other:?}"),
    }
}

#[test]
fn fetch_file_http_error_names_the_url() {
    let a = entry("alpha", "x");
    let server = FakeHttp::start(vec![Route::status("/templates/alpha.md", 500, "boom")]);
    let base = server.base();
    assert_eq!(
        err_text(fetch_file(&Origin::new(&base).unwrap(), &a)),
        format!("I/O error: download of {base}/alpha.md failed: HTTP status 500")
    );
}

#[test]
fn fetch_does_not_follow_redirects() {
    let a = entry("alpha", "x");
    let server = FakeHttp::start(vec![
        Route::redirect("/templates/alpha.md", "/templates/other.md"),
        Route::ok("/templates/other.md", "elsewhere"),
    ]);
    let base = server.base();
    assert_eq!(
        err_text(fetch_file(&Origin::new(&base).unwrap(), &a)),
        format!("I/O error: download of {base}/alpha.md failed: HTTP status 302")
    );
    assert_eq!(server.requests(), vec!["/templates/alpha.md"]);
}

#[test]
fn fetch_sends_a_non_ascii_path_encoded() {
    let a = entry("alpha", "x");
    let server = FakeHttp::start(vec![Route::ok("/caf%C3%A9/alpha.md", "x")]);
    let base = server.base().replace("/templates", "/caf\u{e9}");
    assert_eq!(
        fetch_file(&Origin::new(&base).unwrap(), &a).unwrap(),
        b"x".to_vec()
    );
}

#[test]
fn fetch_refuses_an_invalid_port() {
    let a = entry("alpha", "x");
    let base = "http://127.0.0.1:99999";
    assert_eq!(
        err_text(fetch_file(&Origin::new(base).unwrap(), &a)),
        format!("I/O error: download of {base}/alpha.md failed: invalid port number")
    );
}
