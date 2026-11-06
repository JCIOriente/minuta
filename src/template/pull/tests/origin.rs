use super::*;

#[test]
fn origin_github_url() {
    let origin = Origin::github("main").unwrap();
    assert_eq!(
        origin.url("index.toml"),
        "https://raw.githubusercontent.com/JCIOriente/minuta/main/templates/index.toml"
    );
}

#[test]
fn origin_accepts_ref_forms() {
    for reference in [
        "main",
        "v0.3.0",
        "feature/x-1",
        "release_2",
        "0123456789abcdef0123456789abcdef01234567",
    ] {
        let origin = Origin::github(reference).unwrap();
        assert_eq!(
            origin.url("a.md"),
            format!(
                "https://raw.githubusercontent.com/JCIOriente/minuta/{reference}/templates/a.md"
            ),
            "ref {reference:?}"
        );
    }
}

#[test]
fn origin_rejects_bad_refs() {
    for reference in [
        "", "a b", "../x", "x/../y", "/abs", "a?b", "a#b", "a%2fb", "a\nb", "x/", "a//b", ".", "..",
    ] {
        match Origin::github(reference) {
            Err(Error::Usage(m)) => assert_eq!(
                m,
                format!("invalid ref {reference:?} (use letters, digits, '.', '_', '-' and '/')"),
                "ref {reference:?}"
            ),
            other => panic!("ref {reference:?}: expected usage error, got {other:?}"),
        }
    }
}

#[test]
fn origin_new_rejects_plain_http_elsewhere_and_other_schemes() {
    for base in [
        "http://example.com/t",
        "http://localhost:1/t",
        "http://127.0.0.1.evil.com/t",
        "http://127.0.0.10/t",
        "ftp://example.com/t",
        "file:///tmp/t",
        "example.com/t",
        "",
        "https://",
        "https://user@host.example/t",
        "https://host.example/a b",
        "https://host.example/a\nb",
    ] {
        match Origin::new(base) {
            Err(Error::Usage(m)) => assert_eq!(
                m,
                format!(
                    "templates URL {base:?} must use https (http is only allowed for 127.0.0.1)"
                ),
                "base {base:?}"
            ),
            other => panic!("base {base:?}: expected usage error, got {other:?}"),
        }
    }
}

#[test]
fn origin_new_accepts_https_and_loopback_http() {
    assert_eq!(
        Origin::new("https://example.com/t")
            .unwrap()
            .url("index.toml"),
        "https://example.com/t/index.toml"
    );
    assert_eq!(
        Origin::new("http://127.0.0.1:8080/templates")
            .unwrap()
            .url("index.toml"),
        "http://127.0.0.1:8080/templates/index.toml"
    );
    assert_eq!(
        Origin::new("http://127.0.0.1/templates")
            .unwrap()
            .url("index.toml"),
        "http://127.0.0.1/templates/index.toml"
    );
}

#[test]
fn origin_trailing_slash_is_trimmed() {
    let origin = Origin::new("https://h.example/t/").unwrap();
    assert_eq!(origin.url("a.md"), "https://h.example/t/a.md");
}
