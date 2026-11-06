use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::*;

mod atomic;
mod fingerprints;
mod layout_paths;
mod mark_done;
mod names;
mod open;
mod saved_state;
mod staleness;
mod tmp_cleanup;

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

/// Fingerprints `fp-<step>` for every step, in pipeline order.
fn current() -> Vec<(StepName, String)> {
    StepName::ALL
        .iter()
        .map(|&step| (step, format!("fp-{step}")))
        .collect()
}

/// A job where every step is done with fingerprint `fp-<step>` and artifact `<step>.out`.
fn done_job() -> (TempDir, Job) {
    let (root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    for (step, fp) in current() {
        let artifact = job.dir.join(format!("{step}.out"));
        fs::write(&artifact, step.as_str()).unwrap();
        job.mark_done(step, &fp, 0.1, &[artifact]).unwrap();
    }
    assert!(plan_stale(&job, &current(), None).is_empty());
    (root, job)
}

/// `step` stale for `reason`, and every later step stale because of it.
fn stale_from(step: StepName, reason: Reason) -> Vec<(StepName, Reason)> {
    StepName::ALL
        .into_iter()
        .filter(|&s| s >= step)
        .map(|s| {
            (
                s,
                if s == step {
                    reason.clone()
                } else {
                    Reason::Upstream(step)
                },
            )
        })
        .collect()
}

/// A job where template `summary` is done with fingerprint `fp-summary` and artifact `summary.md`.
fn done_template_job() -> (TempDir, Job) {
    let (root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    fs::write(job.dir.join("summary.md"), "notes").unwrap();
    job.mark_template_done("summary", "fp-summary", 1.5, &[PathBuf::from("summary.md")])
        .unwrap();
    (root, job)
}
