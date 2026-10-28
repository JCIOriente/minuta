//! The `job.json` schema (version 1).

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::write_atomic;
use crate::error::{Error, Result};

/// The only `job.json` version this build reads and writes. Other versions are rejected,
/// never migrated (ADR-0003).
pub const VERSION: u32 = 1;

/// A pipeline step. The order of the variants is the pipeline order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepName {
    Probe,
    Join,
    Audio,
    Transcribe,
    Outputs,
}

impl StepName {
    /// Every step, in pipeline order.
    pub const ALL: [StepName; 5] = [
        StepName::Probe,
        StepName::Join,
        StepName::Audio,
        StepName::Transcribe,
        StepName::Outputs,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            StepName::Probe => "probe",
            StepName::Join => "join",
            StepName::Audio => "audio",
            StepName::Transcribe => "transcribe",
            StepName::Outputs => "outputs",
        }
    }
}

impl fmt::Display for StepName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for StepName {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        StepName::ALL
            .into_iter()
            .find(|name| name.as_str() == s)
            .ok_or_else(|| {
                Error::Usage(format!(
                    "unknown step `{s}`; expected one of probe, join, audio, transcribe, outputs"
                ))
            })
    }
}

/// Whether a step's recorded result can be reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Finished; every artifact in `artifacts` existed when this was written.
    Done,
    /// Must run again; `reason` says why.
    Stale,
}

/// `steps.<name>` in `job.json`. A done state has a fingerprint and seconds and no reason; a
/// stale state has a reason and may keep the fingerprint, seconds and artifacts of its last
/// run. [`JobState::load`] rejects any other combination. (The fields stay public and
/// independent rather than an enum because the runner and staleness checks read them directly.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepState {
    pub status: Status,
    pub fingerprint: Option<String>,
    pub seconds: Option<f64>,
    pub reason: Option<String>,
    /// Artifact paths, relative to the job folder.
    #[serde(default)]
    pub artifacts: Vec<PathBuf>,
}

impl StepState {
    /// Why this combination of fields cannot have been written by a `Job`, if it cannot.
    fn impossible(&self) -> Option<&'static str> {
        match self.status {
            Status::Done if self.fingerprint.is_none() => Some("is done without a fingerprint"),
            Status::Done if self.seconds.is_none() => Some("is done without seconds"),
            Status::Done if self.reason.is_some() => Some("is done but has a stale reason"),
            Status::Stale if self.reason.is_none() => Some("is stale without a reason"),
            Status::Done | Status::Stale => None,
        }
    }
}

/// One source file as last seen: `mtime` is nanoseconds since the Unix epoch and `hash` is
/// the SHA-256 of its content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub path: PathBuf,
    pub size: u64,
    pub mtime: u64,
    pub hash: String,
}

/// The contents of `job.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobState {
    pub version: u32,
    pub name: String,
    pub sources: Vec<Source>,
    pub steps: BTreeMap<StepName, StepState>,
    /// Template steps by template name. Absent in `job.json` files written before templates.
    #[serde(default)]
    pub templates: BTreeMap<String, StepState>,
}

impl JobState {
    pub fn new(name: &str) -> Self {
        JobState {
            version: VERSION,
            name: name.to_owned(),
            sources: Vec::new(),
            steps: BTreeMap::new(),
            templates: BTreeMap::new(),
        }
    }

    /// Reads `job.json`, rejecting any version other than [`VERSION`], unknown top-level fields
    /// and step or template states no `Job` writes (see [`StepState`]).
    pub fn load(path: &Path) -> Result<Self> {
        let invalid = |e: serde_json::Error| Error::Job(format!("invalid {}: {e}", path.display()));
        let value: Value = serde_json::from_slice(&fs::read(path)?).map_err(invalid)?;
        let version = value.get("version").cloned().unwrap_or(Value::Null);
        if version != VERSION {
            return Err(Error::Job(format!(
                "{} has unsupported version {version} (expected {VERSION}); \
                 job folders are never migrated, move or delete it",
                path.display()
            )));
        }
        let state: JobState = serde_json::from_value(value).map_err(invalid)?;
        let steps = state.steps.iter().map(|(n, s)| (format!("step {n}"), s));
        let templates = state
            .templates
            .iter()
            .map(|(n, s)| (format!("template {n}"), s));
        for (what, step) in steps.chain(templates) {
            if let Some(problem) = step.impossible() {
                return Err(Error::Job(format!(
                    "invalid {}: {what} {problem}",
                    path.display()
                )));
            }
        }
        Ok(state)
    }

    /// Writes `job.json` atomically.
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_vec_pretty(self)
            .map_err(|e| Error::Job(format!("cannot serialize {}: {e}", path.display())))?;
        write_atomic(path, |tmp| Ok(fs::write(tmp, json)?))
    }
}
