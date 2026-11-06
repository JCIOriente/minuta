use std::fs;
use std::path::{Path, PathBuf};

use super::*;

#[test]
fn invalid_job_names_rejected() {
    let (_root, out, source) = setup();
    for name in ["", ".", "..", "a/b"] {
        assert!(
            matches!(
                Job::open(&out, name, std::slice::from_ref(&source), false),
                Err(Error::Usage(_))
            ),
            "{name:?}"
        );
    }
    assert!(!out.exists(), "a rejected name must not create anything");
}

#[test]
fn failed_open_touches_nothing() {
    let (root, out, source) = setup();
    let other = root.path().join("other.mp4");
    fs::write(&other, b"other bytes").unwrap();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let leftover = job.dir.join("audio.wav.tmp");
    fs::write(&leftover, b"another run").unwrap();
    let before = fs::read(layout::state_file(&job.dir)).unwrap();

    assert!(Job::open(&out, "clase-4", &[other], false).is_err());
    assert!(leftover.exists(), "a refused open must not clean tmp files");
    assert_eq!(fs::read(layout::state_file(&job.dir)).unwrap(), before);

    let missing = root.path().join("missing.mp4");
    assert!(Job::open(&out, "missing", &[missing], false).is_err());
    assert!(!out.join("missing").exists());
    assert!(leftover.exists());
}

/// `path` relative to `base`, climbing with `..` where needed. Both must be absolute and
/// `base` must hold no symlinks, so `..` from it climbs the spelled-out components.
fn relative_to(path: &Path, base: &Path) -> PathBuf {
    let path: Vec<_> = path.components().collect();
    let base: Vec<_> = base.components().collect();
    let common = path.iter().zip(&base).take_while(|(a, b)| a == b).count();
    let mut relative = PathBuf::new();
    for _ in common..base.len() {
        relative.push("..");
    }
    for component in &path[common..] {
        relative.push(component);
    }
    relative
}

#[test]
fn relative_out_dir_records_artifacts() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
    let out = relative_to(&fs::canonicalize(tmp.path()).unwrap(), &cwd).join("minuta-out");
    assert!(out.is_relative());
    let source = tmp.path().join("clase-4.mp4");
    fs::write(&source, b"source bytes").unwrap();

    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    assert!(job.dir.is_absolute());
    let ctx = job.ctx();
    let artifact = ctx.dir.join("audio.wav");
    fs::write(&artifact, b"wav").unwrap();
    job.mark_done(StepName::Audio, "fp", 0.1, &[artifact])
        .unwrap();

    assert_eq!(
        job.state.steps[&StepName::Audio].artifacts,
        vec![PathBuf::from("audio.wav")]
    );
    let current = vec![(StepName::Audio, "fp".to_string())];
    assert_eq!(plan_stale(&job, &current, None), vec![]);
}

#[test]
fn name_conflict_different_sources() {
    let (root, out, source) = setup();
    let other = root.path().join("sub").join("clase-4.mp4");
    fs::create_dir_all(other.parent().unwrap()).unwrap();
    fs::write(&other, b"other bytes").unwrap();

    let mut job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    job.mark_done(StepName::Probe, "fp", 0.1, &[]).unwrap();

    match Job::open(&out, "clase-4", std::slice::from_ref(&other), false) {
        Err(Error::Job(msg)) => assert!(msg.contains("different sources"), "{msg}"),
        other => panic!("expected Error::Job, got {other:?}"),
    }
    match Job::open(&out, "clase-4", &[source.clone(), other.clone()], false) {
        Err(Error::Job(_)) => {}
        other => panic!("expected Error::Job, got {other:?}"),
    }
    match Job::open(&out, "clase-4", &[other.clone(), source.clone()], false) {
        Err(Error::Job(_)) => {}
        other => panic!("expected Error::Job, got {other:?}"),
    }
    let mut both = Job::open(&out, "both", &[source.clone(), other.clone()], false).unwrap();
    both.mark_done(StepName::Probe, "fp", 0.1, &[]).unwrap();
    match Job::open(&out, "both", &[other.clone(), source.clone()], false) {
        Err(Error::Job(msg)) => assert!(msg.contains("different sources"), "{msg}"),
        other => panic!("source order must count, got {other:?}"),
    }
    let same = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    assert_eq!(same.state.steps.len(), 1, "same sources keep the job");

    let replaced = Job::open(&out, "clase-4", std::slice::from_ref(&other), true).unwrap();
    assert_eq!(replaced.state.sources.len(), 1);
    assert_eq!(
        replaced.state.sources[0].path,
        fs::canonicalize(&other).unwrap()
    );
    assert!(
        replaced.state.steps.is_empty(),
        "replacing starts the job over"
    );
}

#[test]
fn stale_name_refreshed_on_reopen() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    fs::rename(&job.dir, out.join("clase-5")).unwrap();

    let job = Job::open(&out, "clase-5", &[source], false).unwrap();
    assert_eq!(job.state.name, "clase-5");
    assert_eq!(read_json(&layout::state_file(&job.dir))["name"], "clase-5");
}

#[test]
fn flat_layout_job_is_treated_as_new() {
    let (_root, out, source) = setup();
    let dir = out.join("clase-4");
    fs::create_dir_all(&dir).unwrap();
    // A job made by an earlier version, for other sources: it must not conflict.
    let old = br#"{"version":1,"name":"clase-4","sources":[{"path":"/elsewhere.mp4","size":1,"mtime":1}],"steps":{}}"#;
    fs::write(dir.join("job.json"), old).unwrap();
    fs::write(dir.join("transcript.json"), b"old").unwrap();

    let job = Job::open(&out, "clase-4", &[source], false).unwrap();
    assert!(layout::state_file(&job.dir).exists());
    assert_eq!(fs::read(dir.join("job.json")).unwrap(), old);
    assert_eq!(fs::read(dir.join("transcript.json")).unwrap(), b"old");
    assert!(job.state.steps.is_empty());
}
