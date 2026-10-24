use minuta::models::{self, ModelSpec, Pulled};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

const BODY: &[u8] = b"pretend these are model weights";

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

/// Serves `body` to every request on a local port; returns the base URL and a request counter.
fn serve(body: &'static [u8]) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut seen = Vec::new();
            let mut buf = [0u8; 512];
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                seen.extend_from_slice(&buf[..n]);
            }
            counter.fetch_add(1, Ordering::SeqCst);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        }
    });
    (base, hits)
}

fn files_in(dir: &Path) -> Vec<String> {
    match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[test]
fn bad_checksum_rejected_and_cleaned() {
    let (base, _) = serve(BODY);
    let dir = tempfile::tempdir().unwrap();
    let wrong = sha(b"something else");
    let spec = ModelSpec {
        name: "tiny",
        sha256: &wrong,
        size: BODY.len() as u64,
    };
    let err = models::pull(&spec, dir.path(), &base).unwrap_err();
    assert_eq!(err.to_string(), "job error: checksum mismatch for tiny");
    assert!(
        files_in(dir.path()).is_empty(),
        "left {:?}",
        files_in(dir.path())
    );
}

#[test]
fn good_download_renamed() {
    let (base, _) = serve(BODY);
    let dir = tempfile::tempdir().unwrap();
    let good = sha(BODY);
    let spec = ModelSpec {
        name: "tiny",
        sha256: &good,
        size: BODY.len() as u64,
    };
    let out = models::pull(&spec, dir.path(), &base).unwrap();
    assert_eq!(out, Pulled::Downloaded);
    assert_eq!(files_in(dir.path()), vec!["ggml-tiny.bin".to_string()]);
    assert_eq!(
        std::fs::read(dir.path().join("ggml-tiny.bin")).unwrap(),
        BODY
    );
}

#[test]
fn already_present_noop() {
    let (base, hits) = serve(BODY);
    let dir = tempfile::tempdir().unwrap();
    let good = sha(BODY);
    let spec = ModelSpec {
        name: "tiny",
        sha256: &good,
        size: BODY.len() as u64,
    };
    std::fs::write(dir.path().join("ggml-tiny.bin"), BODY).unwrap();
    let out = models::pull(&spec, dir.path(), &base).unwrap();
    assert_eq!(out, Pulled::AlreadyPresent);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[test]
fn invalid_present_file_is_replaced() {
    let (base, _) = serve(BODY);
    let dir = tempfile::tempdir().unwrap();
    let good = sha(BODY);
    let spec = ModelSpec {
        name: "tiny",
        sha256: &good,
        size: BODY.len() as u64,
    };
    std::fs::write(dir.path().join("ggml-tiny.bin"), b"corrupt").unwrap();
    assert_eq!(
        models::pull(&spec, dir.path(), &base).unwrap(),
        Pulled::Downloaded
    );
    assert_eq!(
        std::fs::read(dir.path().join("ggml-tiny.bin")).unwrap(),
        BODY
    );
}

#[test]
fn resolve_path_vs_name() {
    let dir = tempfile::tempdir().unwrap();
    let models_dir = dir.path().join("models");
    let custom = dir.path().join("custom.bin");
    std::fs::write(&custom, b"x").unwrap();
    let given = custom.to_str().unwrap();
    assert_eq!(models::resolve(given, &models_dir).unwrap(), custom);
    assert_eq!(
        models::resolve("small", &models_dir).unwrap(),
        models_dir.join("ggml-small.bin")
    );
    assert_eq!(
        models::resolve("nope", &models_dir)
            .unwrap_err()
            .exit_code(),
        2
    );
}

#[test]
fn catalog_lists_pinned_models() {
    let names: Vec<_> = models::CATALOG.iter().map(|m| m.name).collect();
    assert_eq!(names, ["tiny", "base", "small", "medium", "large-v3"]);
    for m in models::CATALOG {
        assert_eq!(m.sha256.len(), 64);
        assert!(m.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}

#[test]
fn list_shows_installed_and_available_with_sizes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ggml-base.bin"), b"12345").unwrap();
    let rows = models::list(dir.path());
    assert_eq!(rows.len(), 5);
    let base = rows.iter().find(|r| r.name == "base").unwrap();
    assert!(base.installed);
    assert_eq!(base.size, 5);
    let tiny = rows.iter().find(|r| r.name == "tiny").unwrap();
    assert!(!tiny.installed);
    assert_eq!(tiny.size, 77_691_713);
}

#[test]
fn corrupt_present_file_removed_when_download_also_bad() {
    let (base, _) = serve(BODY);
    let dir = tempfile::tempdir().unwrap();
    let wrong = sha(b"something else");
    let spec = ModelSpec {
        name: "tiny",
        sha256: &wrong,
        size: BODY.len() as u64,
    };
    std::fs::write(dir.path().join("ggml-tiny.bin"), b"corrupt").unwrap();
    models::pull(&spec, dir.path(), &base).unwrap_err();
    assert!(
        files_in(dir.path()).is_empty(),
        "left {:?}",
        files_in(dir.path())
    );
}
