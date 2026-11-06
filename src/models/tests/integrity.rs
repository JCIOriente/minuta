//! `pull`: checksum and size checks, and what happens to a file already present.

use super::*;

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
