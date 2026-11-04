use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::job::layout;
use crate::steps::transcribe::TRANSCRIPT_FILE;
use crate::transcript::Transcript;

/// `<name>.md`: in the job folder, or in `assets/` for an implicit template.
pub(super) fn md_path(dir: &Path, name: &str, implicit: bool) -> PathBuf {
    if implicit {
        layout::implicit_md(dir, name)
    } else {
        dir.join(format!("{name}.md"))
    }
}

/// `<name>.json`: always in `assets/`.
pub(super) fn json_path(dir: &Path, name: &str) -> PathBuf {
    layout::template_json(dir, name)
}

pub(super) fn exists(path: &Path) -> Result<bool> {
    path.try_exists().map_err(|e| io_error(path, e))
}

pub(super) fn read_transcript(dir: &Path) -> Result<Transcript> {
    let path = layout::transcript_json(dir);
    let json = fs::read_to_string(&path).map_err(|e| io_error(&path, e))?;
    serde_json::from_str(&json).map_err(|e| Error::Parse(format!("{TRANSCRIPT_FILE}: {e}")))
}

/// Removes `written`, newest first, after `error` stopped the writes, so no partial set of
/// artifacts is left. Returns `error`, with any file that could not be removed added to it.
pub(super) fn remove_written(written: &[PathBuf], error: Error) -> Error {
    let failures: Vec<String> = written
        .iter()
        .rev()
        .filter_map(|path| {
            fs::remove_file(path)
                .err()
                .map(|e| format!("could not remove {}: {e}", path.display()))
        })
        .collect();
    if failures.is_empty() {
        return error;
    }
    let kind = match &error {
        Error::Io(e) => e.kind(),
        _ => io::ErrorKind::Other,
    };
    Error::Io(io::Error::new(
        kind,
        format!("{error}; {}", failures.join("; ")),
    ))
}

pub(super) fn io_error(path: &Path, e: io::Error) -> Error {
    Error::Io(io::Error::new(e.kind(), format!("{}: {e}", path.display())))
}

/// The end of the last segment in whole seconds: the transcript end the chapter rules use.
pub(super) fn end_seconds(t: &Transcript) -> u64 {
    let end = t.segments.last().map_or(0.0, |s| s.end);
    // Truncation to whole seconds is intended; negative ends clamp to zero.
    end.max(0.0) as u64
}

/// `h:mm:ss` at the end of the last segment, truncated to whole seconds.
pub(super) fn duration(t: &Transcript) -> String {
    let secs = end_seconds(t);
    format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}
