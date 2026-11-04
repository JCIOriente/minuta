use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use crate::error::{Error, Result};

mod endpoint;
mod file;

use endpoint::endpoint;
pub use endpoint::Endpoint;
use file::{read_file, FileConfig, Scalar};

const DEFAULT_OUT_DIR: &str = "./minuta-out";
const DEFAULT_LANG: &str = "auto";
/// Kept as written (with `~`) until [`expand_tilde`] runs against the home directory.
const DEFAULT_MODELS_DIR: &str = "~/.minuta/models";
const DEFAULT_ENDPOINT_CONTEXT_TOKENS: &str = "16384";
const DEFAULT_ENDPOINT_TIMEOUT_SECS: &str = "600";

/// Every environment variable minuta reads.
const ENV_NAMES: [&str; 19] = [
    "MINUTA_CONFIG",
    "MINUTA_OUT_DIR",
    "MINUTA_WHISPER_BIN",
    "MINUTA_WHISPER_MODEL",
    "MINUTA_WHISPER_LANG",
    "MINUTA_WHISPER_VAD_MODEL",
    "MINUTA_MODELS_DIR",
    "MINUTA_FFMPEG_BIN",
    "MINUTA_FFPROBE_BIN",
    "MINUTA_LLM",
    "MINUTA_OUTPUT_LANG",
    "MINUTA_LLM_CONCURRENCY",
    "MINUTA_LLM_RETRIES",
    "MINUTA_LLM_BACKOFF_MS",
    "MINUTA_LLM_REPAIRS",
    "MINUTA_OLLAMA_URL",
    "MINUTA_OLLAMA_NUM_CTX",
    "MINUTA_CLAUDE_BIN",
    "MINUTA_CODEX_BIN",
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
    pub llm_default: Option<String>,
    pub llm_output_lang: Option<String>,
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
    /// A path or a catalog name; `None` when unset: whisper runs without VAD.
    pub whisper_vad_model: Option<Setting>,
    /// Where models are kept, with a leading `~` still unexpanded. See [`Config::models_dir_path`].
    pub models_dir: Setting,
    pub ffmpeg_bin: Setting,
    pub ffprobe_bin: Setting,
    pub llm_default: Setting,
    /// `None` when no layer sets a non-empty value: the detected speech language is used.
    pub llm_output_lang: Option<Setting>,
    /// `None` when unset: the backend picks its own default.
    pub llm_concurrency: Option<Setting>,
    pub llm_retries: Setting,
    pub llm_backoff_ms: Setting,
    pub llm_repairs: Setting,
    pub ollama_url: Setting,
    pub ollama_num_ctx: Setting,
    pub claude_bin: Setting,
    pub codex_bin: Setting,
    /// OpenAI-compatible servers by name. Only the config file declares them.
    pub endpoints: BTreeMap<String, Endpoint>,
}

/// Default config location: `~/.minuta/config.toml`.
pub fn default_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| default_path_in(dirs.home_dir()))
}

/// The default config file under `home`: `<home>/.minuta/config.toml`.
pub fn default_path_in(home: &Path) -> PathBuf {
    home.join(".minuta/config.toml")
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
    /// yields defaults. A malformed file is `Error::Config`; an unreadable one is
    /// `Error::ConfigIo`.
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
        let llm = from_file.llm.unwrap_or_default();
        let ollama = from_file.ollama.unwrap_or_default();
        let claude = from_file.claude.unwrap_or_default();
        let codex = from_file.codex.unwrap_or_default();
        let endpoints = from_file
            .endpoints
            .unwrap_or_default()
            .into_iter()
            .map(|(name, table)| Ok((name.clone(), endpoint(&name, table)?)))
            .collect::<Result<_>>()?;

        let config = Config {
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
            whisper_vad_model: pick_opt(None, env("MINUTA_WHISPER_VAD_MODEL"), whisper.vad_model),
            models_dir: pick(
                None,
                env("MINUTA_MODELS_DIR"),
                whisper.models_dir,
                DEFAULT_MODELS_DIR,
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
            llm_default: pick(
                flags.llm_default,
                env("MINUTA_LLM"),
                llm.default,
                "ollama:gemma4:26b",
            ),
            llm_output_lang: pick_opt(
                flags.llm_output_lang,
                env("MINUTA_OUTPUT_LANG"),
                llm.output_lang,
            ),
            llm_concurrency: pick_opt(
                None,
                env("MINUTA_LLM_CONCURRENCY"),
                llm.concurrency.map(Scalar::into_text),
            ),
            llm_retries: pick(
                None,
                env("MINUTA_LLM_RETRIES"),
                llm.retries.map(Scalar::into_text),
                "3",
            ),
            llm_backoff_ms: pick(
                None,
                env("MINUTA_LLM_BACKOFF_MS"),
                llm.backoff_ms.map(Scalar::into_text),
                "2000",
            ),
            llm_repairs: pick(
                None,
                env("MINUTA_LLM_REPAIRS"),
                llm.repairs.map(Scalar::into_text),
                "2",
            ),
            ollama_url: pick(
                None,
                env("MINUTA_OLLAMA_URL"),
                ollama.url,
                "http://127.0.0.1:11434",
            ),
            ollama_num_ctx: pick(
                None,
                env("MINUTA_OLLAMA_NUM_CTX"),
                ollama.num_ctx.map(Scalar::into_text),
                "16384",
            ),
            claude_bin: pick(None, env("MINUTA_CLAUDE_BIN"), claude.bin, "claude"),
            codex_bin: pick(None, env("MINUTA_CODEX_BIN"), codex.bin, "codex"),
            endpoints,
        };
        config.check_numbers()?;
        Ok(config)
    }

    /// Reject numeric settings that the accessors below would not be able to parse.
    /// Runs after every layer is merged, so a bad value is caught whichever layer it came from.
    fn check_numbers(&self) -> Result<()> {
        int_setting::<u32>(&self.llm_retries, "llm.retries", 1)?;
        int_setting::<u64>(&self.llm_backoff_ms, "llm.backoff_ms", 0)?;
        int_setting::<u32>(&self.llm_repairs, "llm.repairs", 0)?;
        int_setting::<usize>(&self.ollama_num_ctx, "ollama.num_ctx", 1)?;
        if let Some(setting) = &self.llm_concurrency {
            int_setting::<usize>(setting, "llm.concurrency", 1)?;
        }
        Ok(())
    }

    /// Retry attempts in total, including the first. Valid by `check_numbers`.
    pub fn retries(&self) -> u32 {
        parse_checked(&self.llm_retries)
    }

    /// Initial delay between attempts; doubles after each failed attempt.
    pub fn backoff(&self) -> Duration {
        Duration::from_millis(parse_checked(&self.llm_backoff_ms))
    }

    /// Repair calls after a final answer breaks a limit; 0 disables repairs. Valid by `check_numbers`.
    pub fn repairs(&self) -> u32 {
        parse_checked(&self.llm_repairs)
    }

    /// Context window requested from Ollama, in tokens.
    pub fn num_ctx(&self) -> usize {
        parse_checked(&self.ollama_num_ctx)
    }

    /// The endpoint called `name`. A missing one is a usage error showing what to add.
    pub fn endpoint(&self, name: &str) -> Result<&Endpoint> {
        self.endpoints.get(name).ok_or_else(|| {
            Error::Usage(format!(
                "unknown endpoint {name:?}; add it to the config file (`minuta config path`):\n  \
                 [endpoints.{name}]\n  \
                 url = \"http://127.0.0.1:1234/v1\"  # your server's base URL"
            ))
        })
    }

    /// Parallel LLM calls, or `None` to let the backend decide.
    pub fn concurrency(&self) -> Option<usize> {
        self.llm_concurrency.as_ref().map(parse_checked)
    }

    /// The models directory with a leading `~` expanded against the home directory.
    pub fn models_dir_path(&self) -> Result<PathBuf> {
        let home = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf());
        expand_tilde(&self.models_dir.value, home.as_deref())
    }

    /// Every key with its resolved setting in documented order; `None` means unset.
    pub fn entries(&self) -> Vec<(&'static str, Option<&Setting>)> {
        vec![
            ("out_dir", Some(&self.out_dir)),
            ("whisper.bin", Some(&self.whisper_bin)),
            ("whisper.model", self.whisper_model.as_ref()),
            ("whisper.lang", Some(&self.whisper_lang)),
            ("whisper.vad_model", self.whisper_vad_model.as_ref()),
            ("whisper.models_dir", Some(&self.models_dir)),
            ("ffmpeg.bin", Some(&self.ffmpeg_bin)),
            ("ffprobe.bin", Some(&self.ffprobe_bin)),
            ("llm.default", Some(&self.llm_default)),
            ("llm.output_lang", self.llm_output_lang.as_ref()),
            ("llm.concurrency", self.llm_concurrency.as_ref()),
            ("llm.retries", Some(&self.llm_retries)),
            ("llm.backoff_ms", Some(&self.llm_backoff_ms)),
            ("llm.repairs", Some(&self.llm_repairs)),
            ("ollama.url", Some(&self.ollama_url)),
            ("ollama.num_ctx", Some(&self.ollama_num_ctx)),
            ("claude.bin", Some(&self.claude_bin)),
            ("codex.bin", Some(&self.codex_bin)),
        ]
    }

    /// Every key of every endpoint by endpoint name; `None` means unset.
    pub fn endpoint_entries(&self) -> Vec<(String, Option<&Setting>)> {
        let mut entries = Vec::new();
        for (name, endpoint) in &self.endpoints {
            for (key, setting) in [
                ("url", Some(&endpoint.url)),
                ("context_tokens", Some(&endpoint.context_tokens)),
                ("timeout_secs", Some(&endpoint.timeout_secs)),
                ("api_key_env", endpoint.api_key_env.as_ref()),
            ] {
                entries.push((format!("endpoints.{name}.{key}"), setting));
            }
        }
        entries
    }
}

/// Expands a leading `~` or `~/` against `home`. Any other value is returned as written,
/// including relative paths. A `~` with no home directory is `Error::Config` naming the key.
pub fn expand_tilde(value: &str, home: Option<&Path>) -> Result<PathBuf> {
    let rest = if value == "~" {
        Some("")
    } else {
        value.strip_prefix("~/")
    };
    match rest {
        None if value.starts_with('~') => Err(Error::Config(format!(
            "whisper.models_dir is {value:?}: only ~ and ~/ expand; use ~/ or an absolute path"
        ))),
        None => Ok(PathBuf::from(value)),
        Some(rest) => {
            let home = home.ok_or_else(|| {
                Error::Config(format!(
                    "whisper.models_dir is {value:?} but the home directory is unknown; set an absolute path"
                ))
            })?;
            Ok(home.join(rest))
        }
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

/// Parse a numeric setting that `Config::resolve` has already validated.
fn parse_checked<T: FromStr>(setting: &Setting) -> T {
    setting
        .value
        .parse()
        .unwrap_or_else(|_| panic!("{} was not validated in Config::resolve", setting.value))
}

/// Parse a whole number that must be at least `min`. The message names the key.
fn int_setting<T: FromStr + PartialOrd + std::fmt::Display>(
    setting: &Setting,
    key: &str,
    min: T,
) -> Result<T> {
    setting
        .value
        .parse::<T>()
        .ok()
        .filter(|n| *n >= min)
        .ok_or_else(|| {
            Error::Config(format!(
                "{key} must be an integer of at least {min}, got {:?}",
                setting.value
            ))
        })
}

fn pick(flag: Option<String>, env: Option<String>, file: Option<String>, default: &str) -> Setting {
    pick_opt(flag, env, file).unwrap_or(Setting {
        value: default.to_string(),
        source: Source::Default,
    })
}
