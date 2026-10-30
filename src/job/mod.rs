//! Job folders: naming, `job.json`, fingerprints, staleness and atomic writes.

pub mod fingerprint;
pub mod stale;
pub mod state;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf, MAIN_SEPARATOR};

use crate::error::{Error, Result};

use fingerprint::source_digest;
pub use fingerprint::{fingerprint, source_record, step_fingerprint};
pub use stale::{plan_stale, template_reason, Force, Reason};
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
        let has_state = file
            .try_exists()
            .map_err(|e| Error::Job(format!("cannot check {}: {e}", file.display())))?;
        let mut state = if has_state {
            JobState::load(&file)?
        } else {
            JobState::new(name)
        };
        // The folder may have been renamed since `job.json` was written.
        name.clone_into(&mut state.name);
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

    /// Records `step` as done with `fingerprint`, then saves `job.json`. `artifacts` are
    /// absolute or relative to the job folder and are stored relative to it. Fails with
    /// `Error::Job`, recording nothing, if any artifact does not exist, cannot be checked, or
    /// lies outside the job folder.
    pub fn mark_done(
        &mut self,
        step: StepName,
        fingerprint: &str,
        seconds: f64,
        artifacts: &[PathBuf],
    ) -> Result<()> {
        let done = self.done_state(&format!("step {step}"), fingerprint, seconds, artifacts)?;
        self.state.steps.insert(step, done);
        self.save()
    }

    /// Marks each planned step stale with its reason, then saves `job.json`.
    pub fn mark_stale(&mut self, plan: &[(StepName, Reason)]) -> Result<()> {
        for (step, reason) in plan {
            set_stale(&mut self.state.steps, *step, reason);
        }
        self.save()
    }

    /// Records template `name` as done with `fingerprint`, then saves `job.json`. `artifacts`
    /// are handled and checked as in [`Job::mark_done`].
    pub fn mark_template_done(
        &mut self,
        name: &str,
        fingerprint: &str,
        seconds: f64,
        artifacts: &[PathBuf],
    ) -> Result<()> {
        let done = self.done_state(&format!("template {name}"), fingerprint, seconds, artifacts)?;
        self.state.templates.insert(name.to_owned(), done);
        self.save()
    }

    /// Marks template `name` stale with `reason`, then saves `job.json`.
    pub fn mark_template_stale(&mut self, name: &str, reason: &Reason) -> Result<()> {
        set_stale(&mut self.state.templates, name.to_owned(), reason);
        self.save()
    }

    /// The done state of `what` (`step <name>` or `template <name>`), with every artifact
    /// checked to exist and made relative to the job folder.
    fn done_state(
        &self,
        what: &str,
        fingerprint: &str,
        seconds: f64,
        artifacts: &[PathBuf],
    ) -> Result<StepState> {
        let artifacts = artifacts
            .iter()
            .map(|artifact| self.relative_artifact(what, artifact))
            .collect::<Result<_>>()?;
        Ok(StepState {
            status: Status::Done,
            fingerprint: Some(fingerprint.to_owned()),
            seconds: Some(seconds),
            reason: None,
            artifacts,
        })
    }

    /// `artifact` (absolute, or relative to the job folder) relative to the canonical job folder.
    /// Its parent is canonicalized so that any spelling of a path inside the folder (through a
    /// symlinked prefix or a `..` detour) is recognized, while the artifact itself may be a
    /// symlink.
    fn relative_artifact(&self, what: &str, artifact: &Path) -> Result<PathBuf> {
        let path = self.dir.join(artifact);
        let exists = path
            .try_exists()
            .map_err(|e| Error::Job(format!("cannot check {}: {e}", path.display())))?;
        if !exists {
            return Err(Error::Job(format!(
                "{what} finished without writing {}",
                artifact.display()
            )));
        }
        let outside = || {
            Error::Job(format!(
                "{what} wrote {} outside the job folder {}",
                artifact.display(),
                self.dir.display()
            ))
        };
        let (Some(parent), Some(file_name)) = (path.parent(), path.file_name()) else {
            return Err(outside());
        };
        let canonical = fs::canonicalize(parent)
            .map_err(|e| Error::Job(format!("cannot resolve {}: {e}", parent.display())))?
            .join(file_name);
        match canonical.strip_prefix(&self.dir) {
            Ok(relative) if !relative.as_os_str().is_empty() => Ok(relative.to_path_buf()),
            _ => Err(outside()),
        }
    }

    /// Removes leftover `*.tmp` files from the job folder (after an interrupt, for example).
    pub fn clean_tmp(&self) -> Result<()> {
        remove_tmp_files(&self.dir)
    }
}

/// Marks `key` stale with `reason`, keeping whatever else it recorded.
fn set_stale<K: Ord>(states: &mut BTreeMap<K, StepState>, key: K, reason: &Reason) {
    let state = states.entry(key).or_insert_with(|| StepState {
        status: Status::Stale,
        fingerprint: None,
        seconds: None,
        reason: None,
        artifacts: Vec::new(),
    });
    state.status = Status::Stale;
    state.reason = Some(reason.to_string());
}

fn remove_tmp_files(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        // `DirEntry::file_type` does not follow symlinks, so a planted link is removed itself and
        // a directory named `*.tmp` is left alone.
        if path.extension().is_some_and(|e| e == "tmp") && !entry.file_type()?.is_dir() {
            fs::remove_file(&path).map_err(|e| with_path(&path, e))?;
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
/// A path without a file name is an `Error::Usage`, and so is a file name that is only media
/// extensions (`.mp4`, `.mp4.wav`): stripping would leave nothing, and keeping `.mp4` as the name
/// would hide the job folder and still end in a media extension.
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
        if !is_media_extension(ext) {
            break;
        }
        if stem.is_empty() {
            return Err(Error::Usage(format!(
                "cannot derive a job name from `{}`: \
                 the file name is only a media extension",
                first.display()
            )));
        }
        name.truncate(stem.len());
    }
    Ok(name)
}

fn is_media_extension(ext: &str) -> bool {
    MEDIA_EXTENSIONS.iter().any(|m| ext.eq_ignore_ascii_case(m))
}

/// Writes an artifact atomically: `f` writes to `<path>.tmp`, which is synced to disk and
/// renamed to `path` only if `f` succeeds. On failure the temp file is removed and `path` is
/// left untouched.
///
/// Whatever already sits at the temp path is removed first and `f` is not called if that fails.
/// A job folder can come from elsewhere, and a symlink planted at `<path>.tmp` would otherwise
/// make `f` write through it to the link's target. `remove_file` removes the link itself.
/// `f` creates the file (an external tool may be the one writing it), so it is not `create_new`.
pub fn write_atomic(path: &Path, f: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    let tmp = tmp_path(path);
    match fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(with_path(&tmp, e)),
    }
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

fn with_path(path: &Path, e: io::Error) -> Error {
    Error::Io(io::Error::new(e.kind(), format!("{}: {e}", path.display())))
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut tmp = OsString::from(path.as_os_str());
    tmp.push(".tmp");
    PathBuf::from(tmp)
}
