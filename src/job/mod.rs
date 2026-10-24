//! Job folders: naming, `job.json`, fingerprints, staleness and atomic writes.

pub mod fingerprint;
pub mod state;

use std::path::Path;

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
