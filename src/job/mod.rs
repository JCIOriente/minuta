//! Job folders: naming, `job.json`, fingerprints, staleness and atomic writes.

pub mod fingerprint;
pub mod state;

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

pub use fingerprint::{fingerprint, source_record};
pub use state::{JobState, Source, Status, StepName, StepState};

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
    /// `*.tmp` files, and records the sources in `job.json`.
    ///
    /// If the folder already holds a job made from a different list of source paths, this is
    /// an `Error::Job` unless `allow_replace` is set, in which case the job starts over.
    pub fn open(out: &Path, name: &str, sources: &[PathBuf], allow_replace: bool) -> Result<Job> {
        let dir = out.join(name);
        fs::create_dir_all(&dir)?;
        remove_tmp_files(&dir)?;
        let paths = sources
            .iter()
            .map(fs::canonicalize)
            .collect::<std::io::Result<Vec<_>>>()?;
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
            .map(|p| source_record(p))
            .collect::<Result<_>>()?;
        let job = Job { dir, state };
        job.save()?;
        Ok(job)
    }

    /// Writes `job.json` atomically.
    pub fn save(&self) -> Result<()> {
        self.state.save(&self.dir.join(JOB_FILE))
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
pub fn derive_name(first: &Path) -> String {
    let mut name = first
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    while let Some((stem, ext)) = name.rsplit_once('.') {
        let is_media = MEDIA_EXTENSIONS.iter().any(|m| ext.eq_ignore_ascii_case(m));
        if stem.is_empty() || !is_media {
            break;
        }
        name.truncate(stem.len());
    }
    name
}

/// Writes an artifact atomically: `f` writes to `<path>.tmp`, which is renamed to `path` only
/// if `f` succeeds. On failure the temp file is removed and `path` is left untouched.
pub fn write_atomic(path: &Path, f: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    let tmp = tmp_path(path);
    let result = f(&tmp).and_then(|()| Ok(fs::rename(&tmp, path)?));
    if result.is_err() {
        // Best effort: a temp file that survives is removed the next time the job is opened.
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut tmp = OsString::from(path.as_os_str());
    tmp.push(".tmp");
    PathBuf::from(tmp)
}
