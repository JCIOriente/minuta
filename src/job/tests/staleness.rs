use std::fs;
use std::path::PathBuf;

use super::*;

#[test]
fn fresh_job_steps_not_run() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", &[source], false).unwrap();
    let plan = plan_stale(&job, &current(), None);
    let expected: Vec<_> = StepName::ALL.iter().map(|&s| (s, Reason::NotRun)).collect();
    assert_eq!(plan, expected);
}

#[test]
fn changed_fingerprint_marks_stale() {
    let (_root, mut job) = done_job();
    let mut fps = current();
    fps[2].1 = "fp-audio-new".into();
    let plan = plan_stale(&job, &fps, None);
    assert_eq!(
        plan,
        stale_from(StepName::Audio, Reason::FingerprintChanged)
    );

    job.mark_stale(&plan).unwrap();
    let json = read_json(&layout::state_file(&job.dir));
    assert_eq!(json["steps"]["audio"]["status"], "stale");
    assert_eq!(json["steps"]["audio"]["reason"], "fingerprint changed");
    assert_eq!(
        json["steps"]["outputs"]["reason"],
        "earlier step audio is stale"
    );
    assert_eq!(json["steps"]["join"]["status"], "done");
    assert_eq!(json["steps"]["join"]["reason"], serde_json::Value::Null);
}

#[test]
fn stale_propagates_to_later_steps() {
    let (_root, job) = done_job();
    let mut fps = current();
    fps[0].1 = "fp-probe-new".into();
    let plan = plan_stale(&job, &fps, None);
    assert_eq!(
        plan,
        stale_from(StepName::Probe, Reason::FingerprintChanged)
    );
}

#[test]
fn interrupted_step_stays_stale() {
    let (_root, mut job) = done_job();
    job.mark_stale(&stale_from(StepName::Transcribe, Reason::Forced))
        .unwrap();
    let artifact = job.dir.join("transcribe.out");
    job.mark_done(StepName::Transcribe, "fp-transcribe", 0.1, &[artifact])
        .unwrap();
    // Interrupted before `outputs` finished: its fingerprint and artifact still match.
    let plan = plan_stale(&job, &current(), None);
    assert_eq!(plan, vec![(StepName::Outputs, Reason::Unfinished)]);
}

#[test]
fn missing_artifact_marks_stale() {
    let (_root, job) = done_job();
    fs::remove_file(job.dir.join("join.out")).unwrap();
    let plan = plan_stale(&job, &current(), None);
    assert_eq!(
        plan,
        stale_from(StepName::Join, Reason::MissingArtifact("join.out".into()))
    );
}

#[test]
fn force_all() {
    let (_root, job) = done_job();
    let plan = plan_stale(&job, &current(), Some(Force::All));
    let expected: Vec<_> = StepName::ALL.iter().map(|&s| (s, Reason::Forced)).collect();
    assert_eq!(plan, expected);
}

#[test]
fn force_step_and_later() {
    let (_root, job) = done_job();
    let plan = plan_stale(&job, &current(), Some(Force::From(StepName::Transcribe)));
    assert_eq!(
        plan,
        vec![
            (StepName::Transcribe, Reason::Forced),
            (StepName::Outputs, Reason::Forced),
        ]
    );
}

#[test]
fn template_not_run() {
    let (_root, job) = done_template_job();
    assert_eq!(
        template_reason(&job, "other", "fp", false).unwrap(),
        Some(Reason::NotRun)
    );
}

#[test]
fn template_unfinished() {
    let (_root, mut job) = done_template_job();
    job.mark_template_stale("summary", &Reason::FingerprintChanged)
        .unwrap();
    assert_eq!(
        template_reason(&job, "summary", "fp-summary", false).unwrap(),
        Some(Reason::Unfinished)
    );
}

#[test]
fn template_forced() {
    let (_root, job) = done_template_job();
    assert_eq!(
        template_reason(&job, "summary", "fp-summary", true).unwrap(),
        Some(Reason::Forced)
    );
}

#[test]
fn template_fingerprint_changed() {
    let (_root, job) = done_template_job();
    assert_eq!(
        template_reason(&job, "summary", "other", false).unwrap(),
        Some(Reason::FingerprintChanged)
    );
}

#[test]
fn template_missing_artifact() {
    let (_root, job) = done_template_job();
    fs::remove_file(job.dir.join("summary.md")).unwrap();
    assert_eq!(
        template_reason(&job, "summary", "fp-summary", false).unwrap(),
        Some(Reason::MissingArtifact("summary.md".to_owned()))
    );
}

#[test]
fn template_current_is_none() {
    let (_root, job) = done_template_job();
    assert_eq!(
        template_reason(&job, "summary", "fp-summary", false).unwrap(),
        None
    );
}

#[test]
fn template_reason_surfaces_io_error() {
    let (_root, mut job) = done_template_job();
    // `summary.md` is a regular file, so checking `summary.md/child` fails with NotADirectory.
    job.state.templates.get_mut("summary").unwrap().artifacts =
        vec![PathBuf::from("summary.md/child")];
    let err = template_reason(&job, "summary", "fp-summary", false).unwrap_err();
    assert!(err.to_string().contains("summary.md/child"), "{err}");
}
