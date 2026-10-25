//! Deciding which steps must run again.

use std::fmt;

use super::{Job, Status, StepName, StepState};
use crate::error::{Error, Result};

/// `--force`: every step, or the named step and every later one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Force {
    All,
    From(StepName),
}

/// Why a step must run again. Its `Display` text is stored in `steps.<name>.reason`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// No result is recorded for the step.
    NotRun,
    /// The step was marked stale and never finished afterwards.
    Unfinished,
    /// `--force` covers the step.
    Forced,
    /// The current fingerprint differs from the recorded one.
    FingerprintChanged,
    /// A recorded artifact, relative to the job folder, no longer exists.
    MissingArtifact(String),
    /// An earlier step is stale.
    Upstream(StepName),
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::NotRun => f.write_str("not run before"),
            Reason::Unfinished => f.write_str("previous run did not finish"),
            Reason::Forced => f.write_str("forced"),
            Reason::FingerprintChanged => f.write_str("fingerprint changed"),
            Reason::MissingArtifact(name) => write!(f, "missing artifact {name}"),
            Reason::Upstream(step) => write!(f, "earlier step {step} is stale"),
        }
    }
}

/// Returns the steps of `current` (step, current fingerprint) that must run, in pipeline order,
/// each with the reason. A stale step makes every later step stale.
pub fn plan_stale(
    job: &Job,
    current: &[(StepName, String)],
    force: Option<Force>,
) -> Vec<(StepName, Reason)> {
    let mut current = current.to_vec();
    current.sort_by_key(|(step, _)| *step);
    let mut first_stale = None;
    let mut plan = Vec::new();
    for (step, fingerprint) in current {
        let forced = match force {
            Some(Force::All) => true,
            Some(Force::From(from)) => step >= from,
            None => false,
        };
        let reason = if forced {
            Some(Reason::Forced)
        } else {
            own_reason(job, step, &fingerprint).or(first_stale.map(Reason::Upstream))
        };
        if let Some(reason) = reason {
            first_stale.get_or_insert(step);
            plan.push((step, reason));
        }
    }
    plan
}

/// Why `step` must run, judged by its own recorded state only.
fn own_reason(job: &Job, step: StepName, fingerprint: &str) -> Option<Reason> {
    let recorded = job.state.steps.get(&step);
    recorded_reason(recorded, fingerprint).or_else(|| {
        recorded?
            .artifacts
            .iter()
            .find(|artifact| !job.dir.join(artifact).exists())
            .map(|artifact| Reason::MissingArtifact(artifact.display().to_string()))
    })
}

/// The reasons that need no file access: nothing recorded, not done, or another fingerprint.
fn recorded_reason(recorded: Option<&StepState>, fingerprint: &str) -> Option<Reason> {
    let Some(recorded) = recorded else {
        return Some(Reason::NotRun);
    };
    if recorded.status != Status::Done {
        return Some(Reason::Unfinished);
    }
    if recorded.fingerprint.as_deref() != Some(fingerprint) {
        return Some(Reason::FingerprintChanged);
    }
    None
}

/// Why template `name` must run, judged by its recorded state in `job.json`. `forced` wins over
/// every other reason. An I/O error while checking an artifact is returned, never read as
/// "missing".
pub fn template_reason(
    job: &Job,
    name: &str,
    fingerprint: &str,
    forced: bool,
) -> Result<Option<Reason>> {
    if forced {
        return Ok(Some(Reason::Forced));
    }
    let recorded = job.state.templates.get(name);
    if let Some(reason) = recorded_reason(recorded, fingerprint) {
        return Ok(Some(reason));
    }
    for artifact in recorded.iter().flat_map(|r| &r.artifacts) {
        let path = job.dir.join(artifact);
        let exists = path
            .try_exists()
            .map_err(|e| Error::Job(format!("cannot check {}: {e}", path.display())))?;
        if !exists {
            return Ok(Some(Reason::MissingArtifact(
                artifact.display().to_string(),
            )));
        }
    }
    Ok(None)
}
