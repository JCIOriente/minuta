//! Job folders: naming, `job.json`, fingerprints, staleness and atomic writes.

pub mod fingerprint;
pub mod stale;
pub mod state;

use std::ffi::OsString;
use std::fs::{self, File};
use std::path::{Path, PathBuf, MAIN_SEPARATOR};

use crate::error::{Error, Result};

use fingerprint::source_digest;
pub use fingerprint::{fingerprint, source_record, step_fingerprint};
pub use stale::{plan_stale, Force, Reason};
pub use state::{JobState, Source, Status, StepName, StepState};

/// What a step sees of its job.
#[derive(Debug, Clone)]
pub struct Ctx {
    /// The job folder, where every artifact is written.
    pub dir: PathBuf,
    /// Canonical source paths, in join order.
    pub sources: Vec<PathBuf>,
    /// Digest of the sources' contents, in order.
    pub source_digest: String,
}

/// One stage of a job. The runner fingerprints a step with [`step_fingerprint`], asks
/// [`plan_stale`] whether it must run, and records the result with [`Job::mark_done`].
pub trait Step {
    const NAME: StepName;
    /// Everything besides the sources that determines the outputs, including the version
    /// string of any tool the step runs.
    fn params(&self, ctx: &Ctx) -> Vec<u8>;
    /// The artifacts the step writes, each with [`write_atomic`].
    fn outputs(&self, ctx: &Ctx) -> Vec<PathBuf>;
    fn run(&self, ctx: &Ctx) -> Result<()>;
}

/// File name of the job state inside the job folder.
pub const JOB_FILE: &str = "job.json";

/// A job folder `<out>/<name>/` and its `job.json` state.
#[derive(Debug)]
pub struct Job {
    pub dir: PathBuf,
    pub state: JobState,
}

impl Job {
    /// Opens or creates the job `<out>/<name>` for `sources` (in join order), removing leftover
    /// `*.tmp` files, and records the sources in `job.json`. `Job::dir` is absolute.
    ///
    /// If the folder already holds a job made from a different list of source paths, this is
    /// an `Error::Job` unless `allow_replace` is set, in which case the job starts over. A name
    /// that is empty, `.`, `..` or contains a path separator is an `Error::Usage`. Names,
    /// sources, `job.json` and conflicts are all checked before anything is created or changed.
    pub fn open(out: &Path, name: &str, sources: &[PathBuf], allow_replace: bool) -> Result<Job> {
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', MAIN_SEPARATOR]) {
            return Err(Error::Usage(format!("invalid job name `{name}`")));
        }
        let paths = sources
            .iter()
            .map(fs::canonicalize)
            .collect::<std::io::Result<Vec<_>>>()?;
        let dir = out.join(name);
        let file = dir.join(JOB_FILE);
        let mut state = if file.exists() {
            JobState::load(&file)?
        } else {
            JobState::new(name)
        };
        if !state.sources.iter().map(|s| &s.path).eq(paths.iter()) {
            if !state.sources.is_empty() && !allow_replace {
                return Err(Error::Job(format!(
                    "{} holds a job made from different sources; \
                     choose another name with --name or replace it with --force",
                    dir.display()
                )));
            }
            state = JobState::new(name);
        }
        state.sources = paths
            .iter()
            .enumerate()
            .map(|(i, p)| source_record(p, state.sources.get(i)))
            .collect::<Result<_>>()?;
        fs::create_dir_all(&dir)?;
        let job = Job {
            dir: fs::canonicalize(&dir)?,
            state,
        };
        job.clean_tmp()?;
        job.save()?;
        Ok(job)
    }

    /// Writes `job.json` atomically.
    pub fn save(&self) -> Result<()> {
        self.state.save(&self.dir.join(JOB_FILE))
    }

    /// The context steps of this job run in.
    pub fn ctx(&self) -> Ctx {
        Ctx {
            dir: self.dir.clone(),
            sources: self.state.sources.iter().map(|s| s.path.clone()).collect(),
            source_digest: source_digest(&self.state.sources),
        }
    }

    /// Records `step` as done with `fingerprint`, then saves `job.json`. Fails with
    /// `Error::Job`, recording nothing, if any of `artifacts` does not exist.
    pub fn mark_done(
        &mut self,
        step: StepName,
        fingerprint: &str,
        seconds: f64,
        artifacts: &[PathBuf],
    ) -> Result<()> {
        if let Some(missing) = artifacts.iter().find(|a| !self.dir.join(a).exists()) {
            return Err(Error::Job(format!(
                "step {step} finished without writing {}",
                missing.display()
            )));
        }
        let artifacts = artifacts
            .iter()
            .map(|a| a.strip_prefix(&self.dir).unwrap_or(a).to_path_buf())
            .collect();
        let done = StepState {
            status: Status::Done,
            fingerprint: Some(fingerprint.to_owned()),
            seconds: Some(seconds),
            reason: None,
            artifacts,
        };
        self.state.steps.insert(step, done);
        self.save()
    }

    /// Marks each planned step stale with its reason, then saves `job.json`.
    pub fn mark_stale(&mut self, plan: &[(StepName, Reason)]) -> Result<()> {
        for (step, reason) in plan {
            let state = self.state.steps.entry(*step).or_insert_with(|| StepState {
                status: Status::Stale,
                fingerprint: None,
                seconds: None,
                reason: None,
                artifacts: Vec::new(),
            });
            state.status = Status::Stale;
            state.reason = Some(reason.to_string());
        }
        self.save()
    }

    /// Removes leftover `*.tmp` files from the job folder (after an interrupt, for example).
    pub fn clean_tmp(&self) -> Result<()> {
        remove_tmp_files(&self.dir)
    }
}

fn remove_tmp_files(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "tmp") && entry.file_type()?.is_file() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

/// Extensions stripped from job names, compared case-insensitively.
const MEDIA_EXTENSIONS: [&str; 9] = [
    "mp4", "mkv", "mov", "m4v", "webm", "mp3", "wav", "m4a", "flac",
];

/// Derives a job name from a path (the first source, or the value of `--name`):
/// its file name with trailing media extensions removed, so the name never ends in one.
/// A path without a file name is an `Error::Usage`.
pub fn derive_name(first: &Path) -> Result<String> {
    let mut name = first
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.is_empty() {
        return Err(Error::Usage(format!(
            "cannot derive a job name from `{}`",
            first.display()
        )));
    }
    while let Some((stem, ext)) = name.rsplit_once('.') {
        let is_media = MEDIA_EXTENSIONS.iter().any(|m| ext.eq_ignore_ascii_case(m));
        if stem.is_empty() || !is_media {
            break;
        }
        name.truncate(stem.len());
    }
    Ok(name)
}

/// Writes an artifact atomically: `f` writes to `<path>.tmp`, which is synced to disk and
/// renamed to `path` only if `f` succeeds. On failure the temp file is removed and `path` is
/// left untouched.
pub fn write_atomic(path: &Path, f: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    let tmp = tmp_path(path);
    let result = f(&tmp).and_then(|()| {
        File::open(&tmp)?.sync_all()?;
        fs::rename(&tmp, path)?;
        Ok(())
    });
    if result.is_err() {
        // Best effort: a temp file that survives is removed the next time the job is opened.
        let _ = fs::remove_file(&tmp);
        return result;
    }
    // Best effort: persist the rename. Some platforms cannot sync a directory.
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let _ = File::open(parent.unwrap_or(Path::new("."))).and_then(|d| d.sync_all());
    Ok(())
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut tmp = OsString::from(path.as_os_str());
    tmp.push(".tmp");
    PathBuf::from(tmp)
}
