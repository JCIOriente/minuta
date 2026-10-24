use std::fs;
use std::path::{Path, PathBuf};

use minuta::error::Error;
use minuta::job::{derive_name, fingerprint, write_atomic, Job, StepName};
use tempfile::TempDir;

/// A temp dir holding one source file `clase-4.mp4` and an empty output dir.
fn setup() -> (TempDir, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("clase-4.mp4");
    fs::write(&source, b"source bytes").unwrap();
    let out = root.path().join("minuta-out");
    (root, out, source)
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

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

#[test]
fn step_names_parse_in_pipeline_order() {
    let names: Vec<StepName> = ["probe", "join", "audio", "transcribe", "outputs"]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(names, StepName::ALL);
    assert!(names.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(StepName::Transcribe.to_string(), "transcribe");
    assert!(matches!("render".parse::<StepName>(), Err(Error::Usage(_))));
}

#[test]
fn job_json_has_version_1() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", &[source.clone()], false).unwrap();
    assert_eq!(job.dir, out.join("clase-4"));
    let json = read_json(&job.dir.join("job.json"));
    assert_eq!(json["version"], 1);
    assert_eq!(json["name"], "clase-4");
    let recorded = &json["sources"][0];
    assert_eq!(
        Path::new(recorded["path"].as_str().unwrap()),
        fs::canonicalize(&source).unwrap()
    );
    assert_eq!(recorded["size"], 12);
    assert!(recorded["mtime"].as_u64().is_some());
    assert_eq!(json["steps"], serde_json::json!({}));
}

#[test]
fn unknown_version_rejected() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", &[source.clone()], false).unwrap();
    let file = job.dir.join("job.json");
    let mut json = read_json(&file);
    json["version"] = 2.into();
    let written = serde_json::to_vec(&json).unwrap();
    fs::write(&file, &written).unwrap();

    match Job::open(&out, "clase-4", &[source], false) {
        Err(Error::Job(msg)) => assert!(msg.contains("version 2"), "{msg}"),
        other => panic!("expected Error::Job, got {other:?}"),
    }
    assert_eq!(
        fs::read(&file).unwrap(),
        written,
        "job.json must not be migrated"
    );
}

#[test]
fn tmp_cleaned_on_open() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", &[source.clone()], false).unwrap();
    fs::write(job.dir.join("audio.wav.tmp"), b"partial").unwrap();
    fs::write(job.dir.join("job.json.tmp"), b"partial").unwrap();
    fs::write(job.dir.join("audio.wav"), b"good").unwrap();

    let job = Job::open(&out, "clase-4", &[source], false).unwrap();
    assert!(!job.dir.join("audio.wav.tmp").exists());
    assert!(!job.dir.join("job.json.tmp").exists());
    assert!(job.dir.join("audio.wav").exists());
}

#[test]
fn name_conflict_different_sources() {
    let (root, out, source) = setup();
    let other = root.path().join("sub").join("clase-4.mp4");
    fs::create_dir_all(other.parent().unwrap()).unwrap();
    fs::write(&other, b"other bytes").unwrap();

    let mut job = Job::open(&out, "clase-4", &[source.clone()], false).unwrap();
    job.state.steps.insert(
        StepName::Probe,
        minuta::job::StepState {
            status: minuta::job::Status::Done,
            fingerprint: Some("fp".into()),
            seconds: Some(0.1),
            reason: None,
            artifacts: vec![],
        },
    );
    job.save().unwrap();

    match Job::open(&out, "clase-4", &[other.clone()], false) {
        Err(Error::Job(msg)) => assert!(msg.contains("different sources"), "{msg}"),
        other => panic!("expected Error::Job, got {other:?}"),
    }
    match Job::open(&out, "clase-4", &[source.clone(), other.clone()], false) {
        Err(Error::Job(_)) => {}
        other => panic!("expected Error::Job, got {other:?}"),
    }
    let same = Job::open(&out, "clase-4", &[source.clone()], false).unwrap();
    assert_eq!(same.state.steps.len(), 1, "same sources keep the job");

    let replaced = Job::open(&out, "clase-4", &[other.clone()], true).unwrap();
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
