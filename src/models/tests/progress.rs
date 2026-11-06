//! `pull_with_progress`: the byte counts it reports.

use super::*;

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

#[test]
fn pull_reports_running_totals_over_many_chunks() {
    let body: &'static [u8] = Box::leak(vec![7u8; 200_000].into_boxed_slice());
    let (base, _requests) = serve_at("/ggml-silero-v5.1.2.bin", body);
    let dir = tempfile::tempdir().unwrap();
    let good = sha(body);
    let spec = ModelSpec {
        name: "silero-v5.1.2",
        file: "ggml-silero-v5.1.2.bin",
        kind: ModelKind::Vad,
        base_url: &base,
        sha256: &good,
        size: body.len() as u64,
    };
    let mut seen = Vec::new();
    models::pull_with_progress(&spec, dir.path(), &mut |n| seen.push(n)).unwrap();
    assert!(seen.len() > 1, "one callback for the whole body: {seen:?}");
    assert!(
        seen.windows(2).all(|w| w[0] < w[1]),
        "not increasing: {seen:?}"
    );
    assert_eq!(seen.last(), Some(&(body.len() as u64)));
}
