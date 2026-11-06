//! `pull`: the download itself, redirects, the request URL and where the file lands.

use super::*;

#[test]
fn good_download_renamed() {
    let (base, requests) = serve(BODY);
    let dir = tempfile::tempdir().unwrap();
    let good = sha(BODY);
    let spec = ModelSpec {
        name: "tiny",
        file: "ggml-tiny.bin",
        kind: ModelKind::Whisper,
        base_url: &base,
        sha256: &good,
        size: BODY.len() as u64,
    };
    let out = pull(&spec, dir.path()).unwrap();
    assert_eq!(*requests.lock().unwrap(), ["GET /ggml-tiny.bin HTTP/1.1"]);
    assert_eq!(out, Pulled::Downloaded);
    assert_eq!(files_in(dir.path()), vec!["ggml-tiny.bin".to_string()]);
    assert_eq!(
        std::fs::read(dir.path().join("ggml-tiny.bin")).unwrap(),
        BODY
    );
}

#[test]
fn download_follows_redirects() {
    let server = FakeHttp::start(vec![
        Route::redirect("/templates/ggml-tiny.bin", "/templates/cdn.bin"),
        Route::ok("/templates/cdn.bin", BODY),
    ]);
    let base = server.base();
    let dir = tempfile::tempdir().unwrap();
    let good = sha(BODY);
    let spec = ModelSpec {
        name: "tiny",
        file: "ggml-tiny.bin",
        kind: ModelKind::Whisper,
        base_url: &base,
        sha256: &good,
        size: BODY.len() as u64,
    };
    assert_eq!(pull(&spec, dir.path()).unwrap(), Pulled::Downloaded);
    assert_eq!(
        server.requests(),
        vec!["/templates/ggml-tiny.bin", "/templates/cdn.bin"]
    );
    assert_eq!(
        std::fs::read(dir.path().join("ggml-tiny.bin")).unwrap(),
        BODY
    );
}

/// `hops` redirects in a row, ending at the file.
fn redirect_chain(hops: usize) -> Vec<Route> {
    let mut routes: Vec<Route> = (0..hops)
        .map(|hop| {
            Route::redirect(
                &format!("/templates/hop{hop}"),
                &format!("/templates/hop{}", hop + 1),
            )
        })
        .collect();
    routes.push(Route::ok(&format!("/templates/hop{hops}"), BODY));
    routes
}

fn pull_hop0(server: &FakeHttp, dir: &Path) -> Result<Pulled> {
    let base = server.base();
    let good = sha(BODY);
    let spec = ModelSpec {
        name: "tiny",
        file: "hop0",
        kind: ModelKind::Whisper,
        base_url: &base,
        sha256: &good,
        size: BODY.len() as u64,
    };
    pull(&spec, dir)
}

#[test]
fn download_follows_four_redirects() {
    let server = FakeHttp::start(redirect_chain(4));
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(pull_hop0(&server, dir.path()).unwrap(), Pulled::Downloaded);
    assert_eq!(server.requests().len(), 5);
}

#[test]
fn download_refuses_a_fifth_redirect() {
    let server = FakeHttp::start(redirect_chain(5));
    let dir = tempfile::tempdir().unwrap();
    let err = pull_hop0(&server, dir.path()).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "I/O error: download of {}/hop0 failed: too many redirects",
            server.base()
        )
    );
    assert_eq!(server.requests().len(), 5);
}

#[test]
fn pull_uses_the_spec_file_name() {
    let (base, requests) = serve_at("/ggml-silero-v5.1.2.bin", BODY);
    let dir = tempfile::tempdir().unwrap();
    let good = sha(BODY);
    let spec = ModelSpec {
        name: "silero-v5.1.2",
        file: "ggml-silero-v5.1.2.bin",
        kind: ModelKind::Vad,
        base_url: &base,
        sha256: &good,
        size: BODY.len() as u64,
    };
    assert_eq!(pull(&spec, dir.path()).unwrap(), Pulled::Downloaded);
    assert_eq!(
        *requests.lock().unwrap(),
        ["GET /ggml-silero-v5.1.2.bin HTTP/1.1"]
    );
    assert_eq!(
        files_in(&dir.path().join("vad")),
        vec!["ggml-silero-v5.1.2.bin".to_string()]
    );
}

#[test]
fn pull_creates_vad_subfolder() {
    let (base, _requests) = serve_at("/ggml-silero-v5.1.2.bin", BODY);
    let dir = tempfile::tempdir().unwrap();
    let good = sha(BODY);
    let spec = ModelSpec {
        name: "silero-v5.1.2",
        file: "ggml-silero-v5.1.2.bin",
        kind: ModelKind::Vad,
        base_url: &base,
        sha256: &good,
        size: BODY.len() as u64,
    };
    pull(&spec, dir.path()).unwrap();
    assert_eq!(
        files_in(&dir.path().join("vad")),
        vec!["ggml-silero-v5.1.2.bin".to_string()]
    );
}

#[test]
fn trailing_slash_in_base_url_is_ok() {
    let (base, requests) = serve(BODY);
    let dir = tempfile::tempdir().unwrap();
    let good = sha(BODY);
    let url = format!("{base}/");
    let spec = ModelSpec {
        name: "tiny",
        file: "ggml-tiny.bin",
        kind: ModelKind::Whisper,
        base_url: &url,
        sha256: &good,
        size: BODY.len() as u64,
    };
    assert_eq!(pull(&spec, dir.path()).unwrap(), Pulled::Downloaded);
    assert_eq!(*requests.lock().unwrap(), ["GET /ggml-tiny.bin HTTP/1.1"]);
}

#[test]
fn download_failure_leaves_no_file() {
    let (base, _) = serve(BODY);
    // Port 9 (discard) is never served, unlike a bound-then-dropped port 0 that a parallel
    // fake server can pick up.
    let closed = "http://127.0.0.1:9".to_string();
    let good = sha(BODY);
    // "base" is not served by the test server, so it answers 404.
    for (name, url) in [("base", &base), ("tiny", &closed)] {
        let dir = tempfile::tempdir().unwrap();
        let file = format!("ggml-{name}.bin");
        let spec = ModelSpec {
            name,
            file: &file,
            kind: ModelKind::Whisper,
            base_url: url,
            sha256: &good,
            size: BODY.len() as u64,
        };
        let err = pull(&spec, dir.path()).unwrap_err();
        assert_eq!(err.exit_code(), 1);
        if name == "base" {
            let msg = err.to_string();
            assert!(
                msg.contains("404") && msg.contains("ggml-base.bin"),
                "{msg}"
            );
        }
        assert!(
            files_in(dir.path()).is_empty(),
            "left {:?}",
            files_in(dir.path())
        );
    }
}
