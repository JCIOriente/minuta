use std::fs;

use super::*;

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

#[cfg(unix)]
#[test]
fn write_atomic_does_not_follow_a_planted_tmp_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let victim = dir.path().join("victim.txt");
    fs::write(&victim, b"precious").unwrap();
    let dest = dir.path().join("summary.md");
    fs::write(&dest, b"old").unwrap();
    std::os::unix::fs::symlink(&victim, dir.path().join("summary.md.tmp")).unwrap();

    write_atomic(&dest, |tmp| {
        fs::write(tmp, b"new")?;
        Ok(())
    })
    .unwrap();

    assert_eq!(fs::read(&victim).unwrap(), b"precious");
    assert_eq!(fs::read(&dest).unwrap(), b"new");
    assert!(!dir.path().join("summary.md.tmp").exists());
    assert!(!dir.path().join("summary.md.tmp").is_symlink());
}

#[cfg(unix)]
#[test]
fn write_atomic_does_not_follow_a_dangling_tmp_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("not-yet.txt");
    let dest = dir.path().join("summary.md");
    std::os::unix::fs::symlink(&missing, dir.path().join("summary.md.tmp")).unwrap();

    write_atomic(&dest, |tmp| {
        fs::write(tmp, b"new")?;
        Ok(())
    })
    .unwrap();

    assert!(!missing.exists());
    assert_eq!(fs::read(&dest).unwrap(), b"new");
}

#[test]
fn write_atomic_replaces_a_stale_regular_tmp() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("summary.md");
    fs::write(dir.path().join("summary.md.tmp"), b"stale partial output").unwrap();

    write_atomic(&dest, |tmp| {
        assert!(!tmp.exists(), "the callback must find no temp entry");
        fs::write(tmp, b"new")?;
        Ok(())
    })
    .unwrap();

    assert_eq!(fs::read(&dest).unwrap(), b"new");
    assert!(!dir.path().join("summary.md.tmp").exists());
}

#[test]
fn write_atomic_reports_an_unremovable_tmp_by_path() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("summary.md");
    let tmp = dir.path().join("summary.md.tmp");
    fs::create_dir(&tmp).unwrap();
    fs::write(tmp.join("inside"), b"x").unwrap();

    let mut called = false;
    let err = write_atomic(&dest, |_| {
        called = true;
        Ok(())
    })
    .unwrap_err();

    assert!(!called, "the callback must not run");
    assert!(matches!(err, Error::Io(_)), "{err:?}");
    assert!(
        err.to_string().contains(&tmp.display().to_string()),
        "{err}"
    );
    assert!(tmp.join("inside").exists());
    assert!(!dest.exists());
}

#[test]
fn write_atomic_creates_missing_parent() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("a").join("b").join("file.txt");
    write_atomic(&path, |tmp| Ok(fs::write(tmp, b"x")?)).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"x");
    assert!(!path.with_extension("txt.tmp").exists());
}
