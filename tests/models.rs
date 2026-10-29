use minuta::models::{self, ModelKind, ModelSpec, Pulled};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex};
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

type Requests = Arc<Mutex<Vec<String>>>;

/// Serves `body` at `/ggml-tiny.bin` (404 for any other path) on a local port; returns the
/// base URL and the request lines received.
fn serve(body: &'static [u8]) -> (String, Requests) {
    serve_at("/ggml-tiny.bin", body)
}

/// Serves `body` at `path` (404 for any other path).
fn serve_at(path: &'static str, body: &'static [u8]) -> (String, Requests) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let requests: Requests = Arc::default();
    let log = requests.clone();
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
            let line = String::from_utf8_lossy(&seen)
                .lines()
                .next()
                .unwrap_or_default()
                .to_string();
            let found = line.starts_with(&format!("GET {path} "));
            log.lock().unwrap().push(line);
            let (status, payload) = if found {
                ("200 OK", body)
            } else {
                ("404 Not Found", &b"missing"[..])
            };
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(payload).unwrap();
        }
    });
    (base, requests)
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
        file: "ggml-tiny.bin",
        kind: ModelKind::Whisper,
        base_url: &base,
        sha256: &wrong,
        size: BODY.len() as u64,
    };
    let err = models::pull(&spec, dir.path()).unwrap_err();
    assert_eq!(err.to_string(), "job error: checksum mismatch for tiny");
    assert!(
        files_in(dir.path()).is_empty(),
        "left {:?}",
        files_in(dir.path())
    );
}

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
    let out = models::pull(&spec, dir.path()).unwrap();
    assert_eq!(*requests.lock().unwrap(), ["GET /ggml-tiny.bin HTTP/1.1"]);
    assert_eq!(out, Pulled::Downloaded);
    assert_eq!(files_in(dir.path()), vec!["ggml-tiny.bin".to_string()]);
    assert_eq!(
        std::fs::read(dir.path().join("ggml-tiny.bin")).unwrap(),
        BODY
    );
}

#[test]
fn already_present_noop() {
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
    std::fs::write(dir.path().join("ggml-tiny.bin"), BODY).unwrap();
    let out = models::pull(&spec, dir.path()).unwrap();
    assert_eq!(out, Pulled::AlreadyPresent);
    assert!(requests.lock().unwrap().is_empty());
}

#[test]
fn invalid_present_file_is_replaced() {
    let (base, _) = serve(BODY);
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
    std::fs::write(dir.path().join("ggml-tiny.bin"), b"corrupt").unwrap();
    assert_eq!(models::pull(&spec, dir.path()).unwrap(), Pulled::Downloaded);
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
    assert_eq!(
        names,
        [
            "tiny",
            "base",
            "small",
            "medium",
            "large-v3",
            "silero-v5.1.2"
        ]
    );
    for m in models::CATALOG {
        assert_eq!(m.sha256.len(), 64);
        assert!(m.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}

fn catalog_spec(name: &str) -> &'static ModelSpec<'static> {
    models::CATALOG.iter().find(|m| m.name == name).unwrap()
}

#[test]
fn catalog_silero_vad_entry() {
    let spec = catalog_spec("silero-v5.1.2");
    assert_eq!(spec.kind, ModelKind::Vad);
    assert_eq!(spec.file, "ggml-silero-v5.1.2.bin");
    assert_eq!(
        spec.base_url,
        "https://huggingface.co/ggml-org/whisper-vad/resolve/main"
    );
    assert_eq!(
        spec.url(),
        "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin"
    );
    assert_eq!(
        spec.sha256,
        "29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf"
    );
    assert_eq!(spec.size, 885_098);
}

#[test]
fn catalog_whisper_entry_keeps_its_url() {
    let spec = catalog_spec("small");
    assert_eq!(spec.kind, ModelKind::Whisper);
    assert_eq!(spec.file, "ggml-small.bin");
    assert_eq!(
        spec.url(),
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin"
    );
}

#[test]
fn vad_model_resolves_into_vad_subfolder() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        models::resolve("silero-v5.1.2", dir.path()).unwrap(),
        dir.path().join("vad").join("ggml-silero-v5.1.2.bin")
    );
}

#[test]
fn whisper_model_resolves_flat() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        models::resolve("tiny", dir.path()).unwrap(),
        dir.path().join("ggml-tiny.bin")
    );
}

#[test]
fn default_dir_is_minuta_models() {
    let home = Path::new("/home/u");
    assert_eq!(
        models::default_dir_for(home),
        Path::new("/home/u/.minuta/models")
    );
}

#[test]
fn old_cache_folder_is_ignored() {
    let home = tempfile::tempdir().unwrap();
    let old = home.path().join(".cache/minuta/models");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("ggml-tiny.bin"), b"old").unwrap();
    let rows = models::list(&models::default_dir_for(home.path())).unwrap();
    let tiny = rows.iter().find(|r| r.name == "tiny").unwrap();
    assert!(!tiny.installed);
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
    assert_eq!(models::pull(&spec, dir.path()).unwrap(), Pulled::Downloaded);
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
    models::pull(&spec, dir.path()).unwrap();
    assert_eq!(
        files_in(&dir.path().join("vad")),
        vec!["ggml-silero-v5.1.2.bin".to_string()]
    );
}

#[test]
fn list_reports_vad_under_vad_subfolder() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("vad")).unwrap();
    std::fs::write(
        dir.path().join("vad").join("ggml-silero-v5.1.2.bin"),
        b"vad",
    )
    .unwrap();
    let rows = models::list(dir.path()).unwrap();
    let vad = rows.iter().find(|r| r.name == "silero-v5.1.2").unwrap();
    assert_eq!(
        *vad,
        models::ModelInfo {
            name: "silero-v5.1.2".into(),
            installed: true,
            size: 3,
        }
    );
}

#[cfg(unix)]
#[test]
fn list_surfaces_metadata_errors_with_the_path() {
    let dir = tempfile::tempdir().unwrap();
    // A symlink to itself: metadata fails with "too many levels of symbolic links", which
    // must not read as "not installed".
    let entry = dir.path().join("ggml-base.bin");
    std::os::unix::fs::symlink(&entry, &entry).unwrap();
    let loop_error = std::fs::metadata(&entry).unwrap_err();
    let err = models::list(dir.path()).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!("I/O error: {}: {loop_error}", entry.display())
    );
}

#[test]
fn list_shows_installed_and_available_with_sizes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ggml-base.bin"), b"12345").unwrap();
    let rows = models::list(dir.path()).unwrap();
    assert_eq!(rows.len(), 6);
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
        file: "ggml-tiny.bin",
        kind: ModelKind::Whisper,
        base_url: &base,
        sha256: &wrong,
        size: BODY.len() as u64,
    };
    std::fs::write(dir.path().join("ggml-tiny.bin"), b"corrupt").unwrap();
    models::pull(&spec, dir.path()).unwrap_err();
    assert!(
        files_in(dir.path()).is_empty(),
        "left {:?}",
        files_in(dir.path())
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
    assert_eq!(models::pull(&spec, dir.path()).unwrap(), Pulled::Downloaded);
    assert_eq!(*requests.lock().unwrap(), ["GET /ggml-tiny.bin HTTP/1.1"]);
}

#[test]
fn oversized_download_rejected_and_cleaned() {
    let (base, _) = serve(BODY);
    let dir = tempfile::tempdir().unwrap();
    let good = sha(BODY);
    let spec = ModelSpec {
        name: "tiny",
        file: "ggml-tiny.bin",
        kind: ModelKind::Whisper,
        base_url: &base,
        sha256: &good,
        size: BODY.len() as u64 - 5,
    };
    let err = models::pull(&spec, dir.path()).unwrap_err();
    assert_eq!(err.to_string(), "job error: size mismatch for tiny");
    assert!(
        files_in(dir.path()).is_empty(),
        "left {:?}",
        files_in(dir.path())
    );
}

#[test]
fn download_failure_leaves_no_file() {
    let (base, _) = serve(BODY);
    let closed = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", l.local_addr().unwrap())
    };
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
        let err = models::pull(&spec, dir.path()).unwrap_err();
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

#[test]
fn resolve_does_not_accept_a_directory() {
    let dir = tempfile::tempdir().unwrap();
    let models_dir = dir.path().join("models");
    // A directory is not a model file, so it must not be returned as is.
    let lookalike = dir.path().join("small");
    std::fs::create_dir(&lookalike).unwrap();
    assert_eq!(
        models::resolve(lookalike.to_str().unwrap(), &models_dir)
            .unwrap_err()
            .exit_code(),
        2
    );
}

#[test]
fn undersized_download_rejected_and_cleaned() {
    let (base, _) = serve(BODY);
    let dir = tempfile::tempdir().unwrap();
    let good = sha(BODY);
    let spec = ModelSpec {
        name: "tiny",
        file: "ggml-tiny.bin",
        kind: ModelKind::Whisper,
        base_url: &base,
        sha256: &good,
        size: BODY.len() as u64 + 5,
    };
    let err = models::pull(&spec, dir.path()).unwrap_err();
    assert_eq!(err.to_string(), "job error: size mismatch for tiny");
    assert!(
        files_in(dir.path()).is_empty(),
        "left {:?}",
        files_in(dir.path())
    );
}

#[test]
fn pull_reports_bytes_up_to_the_size() {
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
    let mut seen = Vec::new();
    let result = models::pull_with_progress(&spec, dir.path(), &mut |n| seen.push(n)).unwrap();
    assert_eq!(result, Pulled::Downloaded);
    assert_eq!(seen.last(), Some(&(BODY.len() as u64)));
}

#[test]
fn pull_already_present_reports_no_bytes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("vad")).unwrap();
    std::fs::write(dir.path().join("vad").join("ggml-silero-v5.1.2.bin"), BODY).unwrap();
    let good = sha(BODY);
    let spec = ModelSpec {
        name: "silero-v5.1.2",
        file: "ggml-silero-v5.1.2.bin",
        kind: ModelKind::Vad,
        base_url: "http://127.0.0.1:9",
        sha256: &good,
        size: BODY.len() as u64,
    };
    let mut seen = Vec::new();
    models::pull_with_progress(&spec, dir.path(), &mut |n| seen.push(n)).unwrap();
    assert!(seen.is_empty());
}
