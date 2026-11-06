use std::fs;
use std::path::{Path, PathBuf};

use super::*;

#[test]
fn job_json_has_version_1() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    assert_eq!(job.dir, fs::canonicalize(out.join("clase-4")).unwrap());
    let json = read_json(&layout::state_file(&job.dir));
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
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let file = layout::state_file(&job.dir);
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
fn missing_version_rejected() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let file = layout::state_file(&job.dir);
    let mut json = read_json(&file);
    json.as_object_mut().unwrap().remove("version");
    let written = serde_json::to_vec(&json).unwrap();
    fs::write(&file, &written).unwrap();

    assert!(matches!(
        Job::open(&out, "clase-4", &[source], false),
        Err(Error::Job(_))
    ));
    assert_eq!(fs::read(&file).unwrap(), written);
}

#[test]
fn invalid_job_json_rejected() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let file = layout::state_file(&job.dir);
    fs::write(&file, b"{ not json").unwrap();

    match Job::open(&out, "clase-4", &[source], false) {
        Err(Error::Job(msg)) => assert!(msg.contains("job.json"), "{msg}"),
        other => panic!("expected Error::Job, got {other:?}"),
    }
    assert_eq!(fs::read(&file).unwrap(), b"{ not json");
}

#[test]
fn old_job_json_loads_without_templates() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let file = layout::state_file(&job.dir);
    let mut json = read_json(&file);
    json.as_object_mut().unwrap().remove("templates");
    fs::write(&file, serde_json::to_vec(&json).unwrap()).unwrap();
    let job = Job::open(&out, "clase-4", &[source], false).unwrap();
    assert!(job.state.templates.is_empty());
    let saved = read_json(&file);
    assert_eq!(saved["templates"], serde_json::json!({}));
    assert_eq!(saved["version"], 1);
}

/// Builds a job with step `audio` and template `summary` done, lets `edit` change its saved
/// `job.json`, and returns the error of reopening it with the `job.json` path `Job::open` reads.
fn reopen_after_edit(edit: impl FnOnce(&mut serde_json::Value)) -> (String, PathBuf) {
    let (root, mut job) = done_template_job();
    let artifact = job.dir.join("audio.wav");
    fs::write(&artifact, b"wav").unwrap();
    job.mark_done(StepName::Audio, "fp-audio", 0.1, &[artifact])
        .unwrap();
    let file = layout::state_file(&job.dir);
    let mut json = read_json(&file);
    edit(&mut json);
    fs::write(&file, serde_json::to_vec(&json).unwrap()).unwrap();

    let out = root.path().join("minuta-out");
    let source = root.path().join("clase-4.mp4");
    let err = Job::open(&out, "clase-4", &[source], false).unwrap_err();
    (err.to_string(), layout::state_file(&out.join("clase-4")))
}

/// Asserts that reopening after `edit` fails with exactly `invalid <job.json>: <problem>; move
/// or delete it`.
fn assert_state_rejected(edit: impl FnOnce(&mut serde_json::Value), problem: &str) {
    let (err, file) = reopen_after_edit(edit);
    assert_eq!(
        err,
        format!(
            "job error: invalid {}: {problem}; move or delete it",
            file.display()
        )
    );
}

#[test]
fn step_done_without_fingerprint_rejected() {
    assert_state_rejected(
        |json| json["steps"]["audio"]["fingerprint"] = serde_json::Value::Null,
        "step audio is done without a fingerprint",
    );
}

#[test]
fn step_done_without_seconds_rejected() {
    assert_state_rejected(
        |json| json["steps"]["audio"]["seconds"] = serde_json::Value::Null,
        "step audio is done without seconds",
    );
}

#[test]
fn step_done_with_reason_rejected() {
    assert_state_rejected(
        |json| json["steps"]["audio"]["reason"] = "forced".into(),
        "step audio is done but has a stale reason",
    );
}

#[test]
fn step_stale_without_reason_rejected() {
    assert_state_rejected(
        |json| json["steps"]["audio"]["status"] = "stale".into(),
        "step audio is stale without a reason",
    );
}

#[test]
fn template_done_without_fingerprint_rejected() {
    assert_state_rejected(
        |json| json["templates"]["summary"]["fingerprint"] = serde_json::Value::Null,
        "template summary is done without a fingerprint",
    );
}

#[test]
fn template_done_without_seconds_rejected() {
    assert_state_rejected(
        |json| json["templates"]["summary"]["seconds"] = serde_json::Value::Null,
        "template summary is done without seconds",
    );
}

#[test]
fn template_done_with_reason_rejected() {
    assert_state_rejected(
        |json| json["templates"]["summary"]["reason"] = "forced".into(),
        "template summary is done but has a stale reason",
    );
}

#[test]
fn template_stale_without_reason_rejected() {
    assert_state_rejected(
        |json| json["templates"]["summary"]["status"] = "stale".into(),
        "template summary is stale without a reason",
    );
}

#[test]
fn step_absolute_artifact_rejected() {
    assert_state_rejected(
        |json| json["steps"]["audio"]["artifacts"] = serde_json::json!(["/elsewhere/audio.wav"]),
        "step audio has artifact /elsewhere/audio.wav outside the job folder",
    );
}

#[test]
fn step_climbing_artifact_rejected() {
    assert_state_rejected(
        |json| json["steps"]["audio"]["artifacts"] = serde_json::json!(["parts/../../x.wav"]),
        "step audio has artifact parts/../../x.wav outside the job folder",
    );
}

#[test]
fn template_absolute_artifact_rejected() {
    assert_state_rejected(
        |json| json["templates"]["summary"]["artifacts"] = serde_json::json!(["/tmp/summary.md"]),
        "template summary has artifact /tmp/summary.md outside the job folder",
    );
}

#[test]
fn stale_step_keeping_old_fingerprint_loads() {
    let (root, mut job) = done_job();
    job.mark_stale(&stale_from(StepName::Audio, Reason::Forced))
        .unwrap();
    let out = root.path().join("minuta-out");
    let source = root.path().join("clase-4.mp4");
    let reopened = Job::open(&out, "clase-4", &[source], false).unwrap();
    assert_eq!(reopened.state, job.state);
}

#[test]
fn unknown_job_state_field_rejected() {
    let (err, file) = reopen_after_edit(|json| {
        json["extra"] = 1.into();
    });
    assert_eq!(
        err,
        format!(
            "job error: invalid {}: unknown field `extra`, \
             expected one of `version`, `name`, `sources`, `steps`, `templates`",
            file.display()
        )
    );
}
