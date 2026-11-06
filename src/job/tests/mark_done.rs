use std::fs;
use std::path::PathBuf;

use crate::error::Error;
use crate::job::{layout, Job, Reason, Status, StepName};

use super::{done_template_job, read_json, setup};

#[test]
fn done_requires_all_artifacts() {
    let (_root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    let present = job.dir.join("audio.wav");
    fs::write(&present, b"wav").unwrap();
    let missing = job.dir.join("audio.json");

    match job.mark_done(StepName::Audio, "fp", 0.1, &[present.clone(), missing]) {
        Err(Error::Job(msg)) => assert!(msg.contains("audio.json"), "{msg}"),
        other => panic!("expected Error::Job, got {other:?}"),
    }
    assert!(!job.state.steps.contains_key(&StepName::Audio));
    let json = read_json(&layout::state_file(&job.dir));
    assert_eq!(json["steps"]["audio"], serde_json::Value::Null);

    job.mark_done(StepName::Audio, "fp", 0.1, &[present])
        .unwrap();
    let json = read_json(&layout::state_file(&job.dir));
    assert_eq!(json["steps"]["audio"]["status"], "done");
    assert_eq!(
        json["steps"]["audio"]["artifacts"],
        serde_json::json!(["audio.wav"])
    );
    assert_eq!(job.state.steps[&StepName::Audio].status, Status::Done);
}

#[test]
fn mark_template_done_requires_artifacts() {
    let (_root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    let err = job
        .mark_template_done("summary", "fp", 1.0, &[PathBuf::from("summary.md")])
        .unwrap_err();
    assert!(matches!(err, Error::Job(_)));
    assert!(job.state.templates.is_empty());
    assert!(read_json(&layout::state_file(&job.dir))["templates"]
        .as_object()
        .unwrap()
        .is_empty());

    let absolute = job.dir.join("summary.md");
    fs::write(&absolute, "notes").unwrap();
    job.mark_template_done("summary", "fp", 1.0, &[absolute])
        .unwrap();
    assert_eq!(
        read_json(&layout::state_file(&job.dir))["templates"]["summary"]["artifacts"],
        serde_json::json!(["summary.md"])
    );
}

#[test]
fn mark_template_stale_records_reason() {
    let (_root, mut job) = done_template_job();
    job.mark_template_stale("summary", &Reason::Forced).unwrap();
    let saved = read_json(&layout::state_file(&job.dir));
    assert_eq!(saved["templates"]["summary"]["status"], "stale");
    assert_eq!(saved["templates"]["summary"]["reason"], "forced");
    job.mark_template_stale("new", &Reason::NotRun).unwrap();
    assert_eq!(job.state.templates["new"].status, Status::Stale);
}

#[test]
fn mark_template_done_surfaces_io_error() {
    let (_root, mut job) = done_template_job();
    let err = job
        .mark_template_done("other", "fp", 1.0, &[PathBuf::from("summary.md/child")])
        .unwrap_err();
    assert!(err.to_string().contains("summary.md/child"), "{err}");
    assert!(!job.state.templates.contains_key("other"));
}

#[test]
fn mark_done_surfaces_io_error() {
    let (_root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    fs::write(job.dir.join("audio.wav"), b"wav").unwrap();
    // `audio.wav` is a regular file, so checking `audio.wav/child` fails with NotADirectory.
    let err = job
        .mark_done(
            StepName::Audio,
            "fp",
            0.1,
            &[PathBuf::from("audio.wav/child")],
        )
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "job error: cannot check {}: Not a directory (os error 20)",
            job.dir.join("audio.wav/child").display()
        )
    );
    assert!(!job.state.steps.contains_key(&StepName::Audio));
}

#[test]
fn mark_done_missing_artifact_names_step() {
    let (_root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    let err = job
        .mark_done(StepName::Audio, "fp", 0.1, &[PathBuf::from("audio.wav")])
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "job error: step audio finished without writing audio.wav"
    );
}

#[test]
fn mark_template_done_missing_artifact_names_template() {
    let (_root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    let err = job
        .mark_template_done("summary", "fp", 1.0, &[PathBuf::from("summary.md")])
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "job error: template summary finished without writing summary.md"
    );
}

#[test]
fn mark_done_rejects_artifact_outside_job_dir() {
    let (root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    let outside = root.path().join("outside.wav");
    fs::write(&outside, b"wav").unwrap();
    let err = job
        .mark_done(StepName::Audio, "fp", 0.1, std::slice::from_ref(&outside))
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "job error: step audio wrote {} outside the job folder {}",
            outside.display(),
            job.dir.display()
        )
    );
    assert!(!job.state.steps.contains_key(&StepName::Audio));
    assert_eq!(
        read_json(&layout::state_file(&job.dir))["steps"],
        serde_json::json!({})
    );
}

#[test]
fn mark_done_rejects_relative_artifact_escaping_job_dir() {
    let (root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    fs::write(root.path().join("outside.wav"), b"wav").unwrap();
    let escaping = PathBuf::from("../../outside.wav");
    let err = job
        .mark_done(StepName::Audio, "fp", 0.1, std::slice::from_ref(&escaping))
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "job error: step audio wrote ../../outside.wav outside the job folder {}",
            job.dir.display()
        )
    );
}

#[test]
fn mark_template_done_rejects_artifact_outside_job_dir() {
    let (root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    let outside = root.path().join("summary.md");
    fs::write(&outside, b"notes").unwrap();
    let err = job
        .mark_template_done("summary", "fp", 1.0, std::slice::from_ref(&outside))
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "job error: template summary wrote {} outside the job folder {}",
            outside.display(),
            job.dir.display()
        )
    );
    assert!(job.state.templates.is_empty());
}

#[test]
fn mark_done_relativizes_non_canonical_inside_path() {
    let (_root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    fs::write(job.dir.join("audio.wav"), b"wav").unwrap();
    // `out` is not canonical on every platform (macOS temp dirs sit behind `/var`), and the
    // `..` detour makes this spelling differ from `job.dir` everywhere.
    let spelled = out
        .join("clase-4")
        .join("..")
        .join("clase-4")
        .join("audio.wav");
    job.mark_done(StepName::Audio, "fp", 0.1, &[spelled])
        .unwrap();
    assert_eq!(
        job.state.steps[&StepName::Audio].artifacts,
        vec![PathBuf::from("audio.wav")]
    );
}

#[test]
fn mark_done_relativizes_nested_relative_path() {
    let (_root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    fs::create_dir(job.dir.join("parts")).unwrap();
    fs::write(job.dir.join("parts").join("a.wav"), b"wav").unwrap();
    job.mark_done(StepName::Audio, "fp", 0.1, &[PathBuf::from("parts/a.wav")])
        .unwrap();
    assert_eq!(
        job.state.steps[&StepName::Audio].artifacts,
        vec![PathBuf::from("parts/a.wav")]
    );
}
