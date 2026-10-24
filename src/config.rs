use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};

const DEFAULT_OUT_DIR: &str = "./minuta-out";
const DEFAULT_LANG: &str = "auto";

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
struct FileConfig {
    out_dir: Option<String>,
    whisper: Option<ToolTable>,
    ffmpeg: Option<ToolTable>,
    ffprobe: Option<ToolTable>,
}

#[derive(Debug, Default, Deserialize)]
struct ToolTable {
    bin: Option<String>,
    model: Option<String>,
    lang: Option<String>,
}

/// Default config location: `~/.config/minuta/config.toml`.
pub fn default_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".config/minuta/config.toml"))
}

/// The config file path: `MINUTA_CONFIG` if set, else the default. Does not read the file.
pub fn resolve_path(env: &dyn Fn(&str) -> Option<String>, file: Option<&str>) -> Option<PathBuf> {
    env("MINUTA_CONFIG")
        .or_else(|| file.map(str::to_string))
        .map(PathBuf::from)
}

impl Config {
    /// Resolve the config using the real process environment and default path.
    pub fn load(flags: Overrides) -> Result<Config> {
        let default = default_path();
        let default = default.as_deref().and_then(Path::to_str);
        Config::resolve(flags, &|k| std::env::var(k).ok(), default)
    }

    /// Merge flags > `MINUTA_*` env > config file > defaults, per key.
    ///
    /// `file` is the default config path; `MINUTA_CONFIG` in `env` overrides it.
    /// A missing file yields defaults; a malformed or unreadable file is `Error::Config`.
    /// Empty values count as unset.
    pub fn resolve(
        flags: Overrides,
        env: &dyn Fn(&str) -> Option<String>,
        file: Option<&str>,
    ) -> Result<Config> {
        let path = resolve_path(env, file);
        let from_file = match &path {
            Some(p) => read_file(p)?,
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

fn read_file(path: &Path) -> Result<FileConfig> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(FileConfig::default()),
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
