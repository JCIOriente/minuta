use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FileConfig {
    pub(super) out_dir: Option<String>,
    pub(super) whisper: Option<ToolTable>,
    pub(super) ffmpeg: Option<BinTable>,
    pub(super) ffprobe: Option<BinTable>,
    pub(super) llm: Option<LlmTable>,
    pub(super) ollama: Option<OllamaTable>,
    pub(super) claude: Option<BinTable>,
    pub(super) codex: Option<BinTable>,
    pub(super) endpoints: Option<BTreeMap<String, EndpointTable>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LlmTable {
    pub(super) default: Option<String>,
    pub(super) output_lang: Option<String>,
    pub(super) concurrency: Option<Scalar>,
    pub(super) retries: Option<Scalar>,
    pub(super) backoff_ms: Option<Scalar>,
    pub(super) repairs: Option<Scalar>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OllamaTable {
    pub(super) url: Option<String>,
    pub(super) num_ctx: Option<Scalar>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EndpointTable {
    pub(super) url: Option<String>,
    pub(super) context_tokens: Option<Scalar>,
    pub(super) timeout_secs: Option<Scalar>,
    pub(super) api_key_env: Option<String>,
}

/// A numeric key may be written as a TOML integer or as a string. Any other TOML value
/// (float, bool, ...) is kept as its text, so `Config::resolve` rejects it by key name
/// like any other non-integer.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum Scalar {
    Int(i64),
    Text(String),
    Other(toml::Value),
}

impl Scalar {
    pub(super) fn into_text(self) -> String {
        match self {
            Scalar::Int(n) => n.to_string(),
            Scalar::Text(text) => text,
            // toml's Display wraps a datetime in its private marker table; the datetime itself
            // is what the user wrote.
            Scalar::Other(toml::Value::Datetime(date)) => date.to_string(),
            Scalar::Other(value) => value.to_string(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ToolTable {
    pub(super) bin: Option<String>,
    pub(super) model: Option<String>,
    pub(super) lang: Option<String>,
    pub(super) vad_model: Option<String>,
    pub(super) models_dir: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BinTable {
    pub(super) bin: Option<String>,
}

/// Read and parse a config file. `explicit` is true when the path came from `MINUTA_CONFIG`,
/// where a missing file is an error rather than a fallback to defaults.
pub(super) fn read_file(path: &Path, explicit: bool) -> Result<FileConfig> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !explicit => {
            return Ok(FileConfig::default())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::Config(format!("{}: not found", path.display())))
        }
        Err(source) => {
            return Err(Error::ConfigIo {
                path: path.to_path_buf(),
                source,
            })
        }
    };
    toml::from_str(&text).map_err(|e| {
        let line = e
            .span()
            .map(|s| text[..s.start].matches('\n').count() + 1)
            .unwrap_or(1);
        Error::Config(format!(
            "{}: invalid TOML at line {line}: {}",
            path.display(),
            e.message()
        ))
    })
}
