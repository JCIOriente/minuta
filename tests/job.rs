use std::fs;
use std::path::Path;

use minuta::error::Error;
use minuta::job::{derive_name, fingerprint, write_atomic};

#[test]
fn derive_name_strips_extension() {
    assert_eq!(derive_name(Path::new("clase-4.mp4")), "clase-4");
    assert_eq!(derive_name(Path::new("/videos/clase-4.mp4")), "clase-4");
    assert_eq!(derive_name(Path::new("clase-4")), "clase-4");
    assert_eq!(derive_name(Path::new("a.b.mkv")), "a.b");
    assert_eq!(derive_name(Path::new("x.mp4.wav")), "x");
    for ext in [
        "mp4", "mkv", "mov", "m4v", "webm", "mp3", "wav", "m4a", "flac",
    ] {
        assert_eq!(derive_name(Path::new(&format!("talk.{ext}"))), "talk");
    }
    assert_eq!(derive_name(Path::new("notes.txt")), "notes.txt");
}

#[test]
fn derive_name_case_insensitive() {
    assert_eq!(derive_name(Path::new("CLASE-4.MP4")), "CLASE-4");
    assert_eq!(derive_name(Path::new("talk.Mov")), "talk");
    assert_eq!(derive_name(Path::new("talk.FlAc")), "talk");
}

#[test]
fn fingerprint_is_stable_and_separates_parts() {
    let a = fingerprint(&[b"ab", b"c"]);
    assert_eq!(a, fingerprint(&[b"ab", b"c"]));
    assert_eq!(a.len(), 64);
    assert_ne!(a, fingerprint(&[b"a", b"bc"]));
    assert_ne!(a, fingerprint(&[b"ab", b"d"]));
}

#[test]
fn write_atomic_failure_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let fresh = dir.path().join("audio.wav");
    let result = write_atomic(&fresh, |tmp| {
        fs::write(tmp, b"partial")?;
        Err(Error::Interrupted)
    });
    assert!(matches!(result, Err(Error::Interrupted)));
    assert!(!fresh.exists());
    assert!(!dir.path().join("audio.wav.tmp").exists());

    let kept = dir.path().join("transcript.json");
    fs::write(&kept, b"good").unwrap();
    let result = write_atomic(&kept, |tmp| {
        fs::write(tmp, b"partial")?;
        Err(Error::Interrupted)
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&kept).unwrap(), b"good");
    assert!(!dir.path().join("transcript.json.tmp").exists());
}

#[test]
fn write_atomic_success_renames_tmp() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("transcript.json");
    fs::write(&dest, b"old").unwrap();
    write_atomic(&dest, |tmp| {
        assert_eq!(tmp, dir.path().join("transcript.json.tmp"));
        fs::write(tmp, b"new")?;
        Ok(())
    })
    .unwrap();
    assert_eq!(fs::read(&dest).unwrap(), b"new");
    assert!(!dir.path().join("transcript.json.tmp").exists());
}
