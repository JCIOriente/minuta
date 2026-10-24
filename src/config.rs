use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};

const DEFAULT_OUT_DIR: &str = "./minuta-out";
const DEFAULT_LANG: &str = "auto";

/// Every environment variable minuta reads.
const ENV_NAMES: [&str; 7] = [
    "MINUTA_CONFIG",
    "MINUTA_OUT_DIR",
    "MINUTA_WHISPER_BIN",
    "MINUTA_WHISPER_MODEL",
    "MINUTA_WHISPER_LANG",
    "MINUTA_FFMPEG_BIN",
    "MINUTA_FFPROBE_BIN",
];

/// Where a resolved value came from, highest precedence first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Flag,
    Env,
    File,
    Default,
}

/// A resolved value and the layer it was taken from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    pub value: String,
    pub source: Source,
}

/// Values given on the command line. `None` means the flag was not passed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    pub out_dir: Option<String>,
    pub whisper_bin: Option<String>,
    pub whisper_model: Option<String>,
    pub whisper_lang: Option<String>,
    pub ffmpeg_bin: Option<String>,
    pub ffprobe_bin: Option<String>,
}

/// The merged configuration, with the file it was read from (if resolved).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub path: Option<PathBuf>,
    pub out_dir: Setting,
    pub whisper_bin: Setting,
    /// `None` when no layer sets a non-empty value.
    pub whisper_model: Option<Setting>,
    pub whisper_lang: Setting,
    pub ffmpeg_bin: Setting,
    pub ffprobe_bin: Setting,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    out_dir: Option<String>,
    whisper: Option<ToolTable>,
    ffmpeg: Option<BinTable>,
    ffprobe: Option<BinTable>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolTable {
    bin: Option<String>,
    model: Option<String>,
    lang: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct BinTable {
    bin: Option<String>,
}

/// Default config location: `~/.config/minuta/config.toml`.
pub fn default_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".config/minuta/config.toml"))
}

/// Read every `MINUTA_*` variable up front. A non-UTF-8 value is `Error::Config`
/// naming the variable; an unset one is simply absent from the map.
pub fn env_snapshot(read: &dyn Fn(&str) -> Option<OsString>) -> Result<HashMap<String, String>> {
    let mut vars = HashMap::new();
    for name in ENV_NAMES {
        if let Some(raw) = read(name) {
            let value = raw
                .into_string()
                .map_err(|_| Error::Config(format!("{name} is not valid UTF-8")))?;
            vars.insert(name.to_string(), value);
        }
    }
    Ok(vars)
}

/// `MINUTA_CONFIG` if set and non-empty.
fn env_config_path(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    env("MINUTA_CONFIG")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// The config file path: `MINUTA_CONFIG` if set, else the default. Does not read the file.
pub fn resolve_path(env: &dyn Fn(&str) -> Option<String>, file: Option<&Path>) -> Option<PathBuf> {
    env_config_path(env).or_else(|| file.map(Path::to_path_buf))
}

impl Config {
    /// Resolve the config using the real process environment and default path.
    pub fn load(flags: Overrides) -> Result<Config> {
        let default = default_path();
        let vars = env_snapshot(&|k| std::env::var_os(k))?;
        Config::resolve(flags, &|k| vars.get(k).cloned(), default.as_deref())
    }

    /// Merge flags > `MINUTA_*` env > config file > defaults, per key.
    ///
    /// `file` is the default config path. A `MINUTA_CONFIG` in `env` replaces it and
    /// must exist: a missing explicit file is `Error::Config`. A missing default file
    /// yields defaults. A malformed or unreadable file is `Error::Config`.
    /// Empty values count as unset.
    pub fn resolve(
        flags: Overrides,
        env: &dyn Fn(&str) -> Option<String>,
        file: Option<&Path>,
    ) -> Result<Config> {
        let explicit = env_config_path(env).is_some();
        let path = resolve_path(env, file);
        let from_file = match &path {
            Some(p) => read_file(p, explicit)?,
            None => FileConfig::default(),
        };
        let whisper = from_file.whisper.unwrap_or_default();
        let ffmpeg = from_file.ffmpeg.unwrap_or_default();
        let ffprobe = from_file.ffprobe.unwrap_or_default();

        Ok(Config {
            path,
            out_dir: pick(
                flags.out_dir,
                env("MINUTA_OUT_DIR"),
                from_file.out_dir,
                DEFAULT_OUT_DIR,
            ),
            whisper_bin: pick(
                flags.whisper_bin,
                env("MINUTA_WHISPER_BIN"),
                whisper.bin,
                "whisper-cli",
            ),
            whisper_model: pick_opt(
                flags.whisper_model,
                env("MINUTA_WHISPER_MODEL"),
                whisper.model,
            ),
            whisper_lang: pick(
                flags.whisper_lang,
                env("MINUTA_WHISPER_LANG"),
                whisper.lang,
                DEFAULT_LANG,
            ),
            ffmpeg_bin: pick(
                flags.ffmpeg_bin,
                env("MINUTA_FFMPEG_BIN"),
                ffmpeg.bin,
                "ffmpeg",
            ),
            ffprobe_bin: pick(
                flags.ffprobe_bin,
                env("MINUTA_FFPROBE_BIN"),
                ffprobe.bin,
                "ffprobe",
            ),
        })
    }

    /// Every key with its resolved setting in documented order; `None` means unset.
    pub fn entries(&self) -> Vec<(&'static str, Option<&Setting>)> {
        vec![
            ("out_dir", Some(&self.out_dir)),
            ("whisper.bin", Some(&self.whisper_bin)),
            ("whisper.model", self.whisper_model.as_ref()),
            ("whisper.lang", Some(&self.whisper_lang)),
            ("ffmpeg.bin", Some(&self.ffmpeg_bin)),
            ("ffprobe.bin", Some(&self.ffprobe_bin)),
        ]
    }
}

/// The highest-precedence non-empty layer, or `None` if every layer is unset or empty.
fn pick_opt(flag: Option<String>, env: Option<String>, file: Option<String>) -> Option<Setting> {
    [
        (flag, Source::Flag),
        (env, Source::Env),
        (file, Source::File),
    ]
    .into_iter()
    .find_map(|(value, source)| {
        value
            .filter(|v| !v.is_empty())
            .map(|value| Setting { value, source })
    })
}

fn pick(flag: Option<String>, env: Option<String>, file: Option<String>, default: &str) -> Setting {
    pick_opt(flag, env, file).unwrap_or(Setting {
        value: default.to_string(),
        source: Source::Default,
    })
}

/// Read and parse a config file. `explicit` is true when the path came from `MINUTA_CONFIG`,
/// where a missing file is an error rather than a fallback to defaults.
fn read_file(path: &Path, explicit: bool) -> Result<FileConfig> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !explicit => {
            return Ok(FileConfig::default())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::Config(format!("{}: not found", path.display())))
        }
        Err(e) => return Err(Error::Config(format!("{}: {e}", path.display()))),
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
