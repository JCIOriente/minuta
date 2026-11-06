//! `minuta config init`: write a commented template of every config key.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::Path;

use crate::error::{Error, Result};

/// The file `config init` writes. Every key from `Config::entries()` appears once, commented
/// out, with its default (or an example for keys that are unset by default). `[whisper]` is
/// the only live line, so the file parses as an empty config. `src/config_init/tests.rs`
/// checks it against the key list.
pub(crate) const TEMPLATE: &str = r##"# minuta configuration, written by `minuta config init`.
# Every key is commented out and shows its default. Remove the leading "# " to set one.
# MINUTA_* environment variables and command-line flags override this file.

# Folder where job folders are created.
# out_dir = "./minuta-out"

[whisper]
# bin = "whisper-cli"
# Model name or path. Unset by default.
# model = "small"
# Speech language, or "auto" to detect it.
# lang = "auto"
# Voice activity detection model, a name or path. Unset by default: no VAD.
# vad_model = "silero-v5.1.2"
# models_dir = "~/.minuta/models"

# [ffmpeg]
# bin = "ffmpeg"

# [ffprobe]
# bin = "ffprobe"

# [llm]
# default = "ollama:gemma4:26b"
# Language of generated documents. Unset by default: the speech language is used.
# output_lang = "en"
# Parallel LLM calls. Unset by default: the backend decides.
# concurrency = 2
# retries = 3
# backoff_ms = 2000
# repairs = 2

# [ollama]
# url = "http://127.0.0.1:11434"
# num_ctx = 16384

# [claude]
# bin = "claude"

# [codex]
# bin = "codex"

# Any OpenAI-compatible server, such as LM Studio or Colibri. Name it here, then use
# `--llm lmstudio:<model>`. Repeat the table for each server.
#
# [endpoints.lmstudio]
#   Base URL, including /v1. Required.
#   url = "http://127.0.0.1:1234/v1"
#   Prompt and reply budget per call. Must fit the context the server gives the model.
#   context_tokens = 16384
#   Seconds to wait for one reply. Raise it for slow models.
#   timeout_secs = 600
#   The name of the environment variable holding the API key, never the key itself.
#   api_key_env = "MY_SERVER_KEY"
"##;

/// What [`write`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Written {
    /// The file did not exist and was created.
    Created,
    /// The file existed and `force` replaced it.
    Overwrote,
    /// The file existed and was left alone.
    Exists,
}

/// Write [`TEMPLATE`] to `path`, creating its parent folder. An existing file is never read:
/// it is left alone, or replaced when `force` is set. I/O errors name the path involved.
pub(crate) fn write(path: &Path, force: bool) -> Result<Written> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|source| io_error(parent, source))?;
    }
    let (mut file, written) = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => (file, Written::Created),
        Err(e) if e.kind() == ErrorKind::AlreadyExists && !force => return Ok(Written::Exists),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(path)
                .map_err(|source| io_error(path, source))?;
            (file, Written::Overwrote)
        }
        Err(source) => return Err(io_error(path, source)),
    };
    file.write_all(TEMPLATE.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|source| io_error(path, source))?;
    Ok(written)
}

fn io_error(path: &Path, source: std::io::Error) -> Error {
    Error::ConfigIo {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests;
