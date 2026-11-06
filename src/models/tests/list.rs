//! `list`: installed and available models, with sizes.

use super::*;

#[test]
fn old_cache_folder_is_ignored() {
    let home = tempfile::tempdir().unwrap();
    let old = home.path().join(".cache/minuta/models");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("ggml-tiny.bin"), b"old").unwrap();
    let rows = list(&default_dir_for(home.path())).unwrap();
    let tiny = rows.iter().find(|r| r.name == "tiny").unwrap();
    assert!(!tiny.installed);
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
    let rows = list(dir.path()).unwrap();
    let vad = rows.iter().find(|r| r.name == "silero-v5.1.2").unwrap();
    assert_eq!(
        *vad,
        ModelInfo {
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
    let err = list(dir.path()).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!("I/O error: {}: {loop_error}", entry.display())
    );
}

#[test]
fn list_shows_installed_and_available_with_sizes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ggml-base.bin"), b"12345").unwrap();
    let rows = list(dir.path()).unwrap();
    assert_eq!(rows.len(), 6);
    let base = rows.iter().find(|r| r.name == "base").unwrap();
    assert!(base.installed);
    assert_eq!(base.size, 5);
    let tiny = rows.iter().find(|r| r.name == "tiny").unwrap();
    assert!(!tiny.installed);
    assert_eq!(tiny.size, 77_691_713);
}
