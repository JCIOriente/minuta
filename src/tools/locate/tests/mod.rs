use super::*;

#[cfg(unix)]
mod install_hint;
#[cfg(unix)]
mod preflight;

/// Committed and already executable: writing a script here and running it can fail with
/// ETXTBSY on Linux when another test thread forks while the file is open for writing.
const HANG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bin/hang");

#[test]
fn hanging_version_probe_times_out() {
    let start = Instant::now();
    let err = probe_version(Path::new(HANG), "--version", Duration::from_millis(200)).unwrap_err();
    assert!(err.contains("timed out"), "err was {err:?}");
    assert!(start.elapsed() < Duration::from_secs(2));
}
