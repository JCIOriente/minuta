use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use tempfile::TempDir;

use super::*;

#[test]
fn tmp_cleaned_on_open() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    fs::write(job.dir.join("audio.wav.tmp"), b"partial").unwrap();
    fs::write(job.dir.join(".minuta").join("job.json.tmp"), b"partial").unwrap();
    fs::write(job.dir.join("audio.wav"), b"good").unwrap();

    let job = Job::open(&out, "clase-4", &[source], false).unwrap();
    assert!(!job.dir.join("audio.wav.tmp").exists());
    assert!(!job.dir.join(".minuta").join("job.json.tmp").exists());
    assert!(job.dir.join("audio.wav").exists());
}

#[cfg(unix)]
#[test]
fn tmp_cleanup_removes_symlinks_without_following_them() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let victim_dir = tempfile::tempdir().unwrap();
    let victim = victim_dir.path().join("victim.txt");
    fs::write(&victim, b"precious").unwrap();
    let victim_subdir = victim_dir.path().join("subdir");
    fs::create_dir(&victim_subdir).unwrap();
    fs::write(victim_subdir.join("inside"), b"keep").unwrap();
    let dangling = victim_dir.path().join("missing");
    std::os::unix::fs::symlink(&victim, job.dir.join("summary.md.tmp")).unwrap();
    std::os::unix::fs::symlink(&victim_subdir, job.dir.join("dir.tmp")).unwrap();
    std::os::unix::fs::symlink(&dangling, job.dir.join("dangling.tmp")).unwrap();
    fs::write(job.dir.join("audio.wav.tmp"), b"stale").unwrap();
    fs::write(job.dir.join("summary.md"), b"good").unwrap();
    std::os::unix::fs::symlink(&victim, job.dir.join("link-not-tmp")).unwrap();

    job.clean_tmp().unwrap();

    for name in ["summary.md.tmp", "dir.tmp", "dangling.tmp", "audio.wav.tmp"] {
        assert!(
            fs::symlink_metadata(job.dir.join(name)).is_err(),
            "{name} should be gone"
        );
    }
    assert_eq!(fs::read(&victim).unwrap(), b"precious");
    assert_eq!(fs::read(victim_subdir.join("inside")).unwrap(), b"keep");
    assert_eq!(fs::read(job.dir.join("summary.md")).unwrap(), b"good");
    assert!(job.dir.join("link-not-tmp").is_symlink());
}

/// Whether `MINUTA_SKIP_PERMISSION_TESTS` asks to skip: only the exact value `1` does.
fn skip_requested(value: Option<&OsStr>) -> bool {
    value == Some(OsStr::new("1"))
}

#[test]
fn skip_requested_only_for_the_exact_value_1() {
    assert!(!skip_requested(None), "unset");
    assert!(skip_requested(Some(OsStr::new("1"))), "1");
    assert!(!skip_requested(Some(OsStr::new("0"))), "0");
    assert!(!skip_requested(Some(OsStr::new(""))), "empty");
    assert!(!skip_requested(Some(OsStr::new("true"))), "true");
}

#[cfg(unix)]
#[test]
fn tmp_cleanup_reports_an_unremovable_entry_by_path() {
    use std::os::unix::fs::PermissionsExt;

    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", &[source], false).unwrap();
    let planted = job.dir.join("summary.md.tmp");
    std::os::unix::fs::symlink("victim", &planted).unwrap();
    fs::set_permissions(&job.dir, fs::Permissions::from_mode(0o555)).unwrap();
    let result = job.clean_tmp();
    // Running as root: the permission bits do not stop the removal, so there is no error.
    // No other way to make `remove_file` fail on a symlink, so fail loudly, or skip visibly
    // when `MINUTA_SKIP_PERMISSION_TESTS=1`.
    let enforced = fs::File::create(job.dir.join("probe-writable")).is_err();
    fs::set_permissions(&job.dir, fs::Permissions::from_mode(0o755)).unwrap();
    if !enforced {
        if skip_requested(std::env::var_os("MINUTA_SKIP_PERMISSION_TESTS").as_deref()) {
            // Straight to the process stderr: libtest captures `eprintln!` of a passing test.
            let _ = writeln!(
                std::io::stderr(),
                "SKIPPED tmp_cleanup_reports_an_unremovable_entry_by_path: permission bits \
                 are not enforced (running as root?) and MINUTA_SKIP_PERMISSION_TESTS=1"
            );
            return;
        }
        panic!(
            "permission bits are not enforced (running as root?); \
             set MINUTA_SKIP_PERMISSION_TESTS=1 (exactly 1) to skip this test"
        );
    }
    let err = result.unwrap_err();
    assert!(matches!(err, Error::Io(_)), "{err:?}");
    assert!(
        err.to_string().contains(&planted.display().to_string()),
        "{err}"
    );
    assert!(planted.is_symlink());
}

#[cfg(unix)]
#[test]
fn tmp_cleaned_on_open_removes_a_planted_symlink() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let victim_dir = tempfile::tempdir().unwrap();
    let victim = victim_dir.path().join("victim.txt");
    fs::write(&victim, b"precious").unwrap();
    std::os::unix::fs::symlink(&victim, job.dir.join("summary.md.tmp")).unwrap();

    Job::open(&out, "clase-4", &[source], false).unwrap();

    assert!(fs::symlink_metadata(job.dir.join("summary.md.tmp")).is_err());
    assert_eq!(fs::read(&victim).unwrap(), b"precious");
}

#[test]
fn tmp_cleanup_spares_other_names() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    fs::create_dir(job.dir.join("cache.tmp")).unwrap();
    fs::write(job.dir.join("notes.tmpl"), b"keep").unwrap();

    let job = Job::open(&out, "clase-4", &[source], false).unwrap();
    assert!(job.dir.join("cache.tmp").is_dir());
    assert!(job.dir.join("notes.tmpl").exists());
}

#[test]
fn open_cleans_tmp_in_all_tiers() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let assets = job.dir.join(layout::ASSETS_DIR);
    fs::create_dir_all(&assets).unwrap();
    let tmps = [
        job.dir.join("transcript.txt.tmp"),
        assets.join("audio.wav.tmp"),
        layout::state_file(&job.dir).with_extension("json.tmp"),
    ];
    let keep = assets.join("audio.wav");
    fs::write(&keep, b"good").unwrap();
    for tmp in &tmps {
        fs::write(tmp, b"partial").unwrap();
    }

    Job::open(&out, "clase-4", &[source], false).unwrap();
    for tmp in &tmps {
        assert!(!tmp.exists(), "{} should be removed", tmp.display());
    }
    assert!(keep.exists());
}

/// A job whose `tier` folder (`.minuta` or `assets`) is a symlink to a folder outside it
/// holding a `victim.tmp` file. Returns the job, the outside folder and the victim.
fn job_with_symlinked_tier(tier: &str) -> (TempDir, Job, PathBuf, PathBuf) {
    let (root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let victim = outside.join("victim.tmp");
    fs::write(&victim, b"keep").unwrap();
    let link = job.dir.join(tier);
    if link.is_dir() {
        fs::remove_dir_all(&link).unwrap();
    }
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    (root, job, outside, victim)
}

#[test]
fn clean_tmp_refuses_a_symlinked_tier() {
    for tier in [layout::STATE_DIR, layout::ASSETS_DIR] {
        let (_root, job, _outside, victim) = job_with_symlinked_tier(tier);
        let err = job.clean_tmp().unwrap_err();
        assert!(matches!(err, Error::Job(_)), "{tier}: {err}");
        assert!(err.to_string().contains(tier), "{err}");
        assert_eq!(fs::read(&victim).unwrap(), b"keep", "{tier}");
    }
}

#[test]
fn write_atomic_refuses_a_symlinked_parent() {
    let (_root, job, outside, _victim) = job_with_symlinked_tier(layout::ASSETS_DIR);
    let path = layout::audio_file(&job.dir);
    let err = write_atomic_in(&job.dir, &path, |tmp| Ok(fs::write(tmp, b"x")?)).unwrap_err();
    assert!(matches!(err, Error::Job(_)), "{err}");
    assert!(!outside.join("audio.wav").exists());
}

#[test]
fn write_atomic_does_not_create_through_a_symlinked_ancestor_tier() {
    let (_root, job, outside, _victim) = job_with_symlinked_tier(layout::STATE_DIR);
    let path = layout::call_cache_dir(&job.dir).join("k.txt");
    let err = write_atomic_in(&job.dir, &path, |tmp| Ok(fs::write(tmp, b"x")?)).unwrap_err();
    assert!(matches!(err, Error::Job(_)), "{err}");
    assert!(!outside.join("llm-cache").exists());
}

#[test]
fn job_folders_named_like_tiers_or_inside_symlinks_work() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    fs::create_dir(&real).unwrap();
    let source = root.path().join("clase-4.mp4");
    fs::write(&source, b"source bytes").unwrap();
    // `out` itself is a symlink named `assets`, and the job folder is a symlink named `.minuta`.
    let linked_out = root.path().join(layout::ASSETS_DIR);
    std::os::unix::fs::symlink(&real, &linked_out).unwrap();
    let job = Job::open(&linked_out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    write_atomic_in(&job.dir, &layout::audio_file(&job.dir), |tmp| {
        Ok(fs::write(tmp, b"x")?)
    })
    .unwrap();
    job.clean_tmp().unwrap();

    let real_job = root.path().join("real-job");
    fs::create_dir(&real_job).unwrap();
    let hidden = root.path().join(layout::STATE_DIR);
    std::os::unix::fs::symlink(&real_job, &hidden).unwrap();
    let job = Job::open(root.path(), layout::STATE_DIR, &[source], false).unwrap();
    assert_eq!(job.dir, fs::canonicalize(&real_job).unwrap());
    write_atomic_in(&job.dir, &layout::transcript_json(&job.dir), |tmp| {
        Ok(fs::write(tmp, b"x")?)
    })
    .unwrap();
    job.save().unwrap();
    job.clean_tmp().unwrap();
}
