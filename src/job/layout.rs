//! Where every file of a job folder lives. Three tiers: documents at the root, machine-readable
//! results and media in `assets/`, minuta's own bookkeeping in the hidden `.minuta/`.
//!
//! Steps and templates take their paths from here and never build `dir.join("...")` by hand.
//! Callers pass the job folder.

use std::path::{Path, PathBuf};

/// Folder of reusable machine files: JSON results, `audio.wav`, the joined recording.
pub const ASSETS_DIR: &str = "assets";
/// Hidden folder of minuta's own state.
pub const STATE_DIR: &str = ".minuta";

fn assets(dir: &Path) -> PathBuf {
    dir.join(ASSETS_DIR)
}

fn state_dir(dir: &Path) -> PathBuf {
    dir.join(STATE_DIR)
}

/// `.minuta/job.json`.
pub fn state_file(dir: &Path) -> PathBuf {
    state_dir(dir).join("job.json")
}

/// `.minuta/probe.json`.
pub fn probe_file(dir: &Path) -> PathBuf {
    state_dir(dir).join("probe.json")
}

/// `.minuta/llm-cache`.
pub fn call_cache_dir(dir: &Path) -> PathBuf {
    state_dir(dir).join("llm-cache")
}

/// `.minuta/derived`: outputs of templates that ran only because another needed them.
pub fn derived_dir(dir: &Path) -> PathBuf {
    state_dir(dir).join("derived")
}

/// `.minuta/whisper-raw.json`, whisper's own output while transcribing. whisper-cli is given
/// the same path without `.json` as its output prefix.
pub fn raw_whisper(dir: &Path) -> PathBuf {
    state_dir(dir).join("whisper-raw.json")
}

/// `assets/transcript.json`.
pub fn transcript_json(dir: &Path) -> PathBuf {
    assets(dir).join("transcript.json")
}

/// `assets/audio.wav`.
pub fn audio_file(dir: &Path) -> PathBuf {
    assets(dir).join("audio.wav")
}

/// `assets/joined.<ext>`.
pub fn joined_file(dir: &Path, ext: &str) -> PathBuf {
    assets(dir).join(format!("joined.{ext}"))
}

/// `assets/<name>.json`, the result of template `name`.
pub fn template_json(dir: &Path, name: &str) -> PathBuf {
    assets(dir).join(format!("{name}.json"))
}
