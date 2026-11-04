use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::error::{Error, Missing, Result};
use crate::llm::claude::{self, Claude};
use crate::llm::codex::{self, Codex};
use crate::llm::endpoint::{self, Problem};
use crate::llm::ollama::{self, Ollama};
use crate::llm::{Backend, BackendId};
use crate::models::{self, ModelKind};
use crate::tools::{self, Report, ToolInfo, ToolPaths, Toolchain};

/// What preflight found: the media tools, the VAD model and a ready backend per backend id
/// in the run.
pub(crate) struct Ready {
    pub(super) toolchain: Toolchain,
    /// `None` when no VAD model is configured.
    pub(super) vad_model: Option<PathBuf>,
    /// Keyed by backend id.
    pub(super) backends: BTreeMap<String, Box<dyn Backend>>,
}

/// Resolves the toolchain for `config` and checks every backend in `backends`, listing every
/// missing tool, the model, a configured VAD model that is not a file and every unreachable
/// backend or missing LLM model at once.
pub(crate) fn preflight(config: &Config, backends: &[BackendId]) -> Result<Ready> {
    let mut missing = Vec::new();
    let toolchain = match tools::preflight(&tool_paths(config)?, true) {
        Ok(toolchain) => Some(toolchain),
        Err(Error::MissingTools(m)) => {
            missing.extend(m);
            None
        }
        Err(other) => return Err(other),
    };
    let vad_model = locate_vad_model(config, &mut missing)?;
    missing = with_pull_hint(missing, config);
    let mut ready: BTreeMap<String, Box<dyn Backend>> = BTreeMap::new();
    let mut claude_bin: Option<std::result::Result<PathBuf, Missing>> = None;
    let mut codex_bin: Option<std::result::Result<PathBuf, Missing>> = None;
    for id in backends {
        let backend: std::result::Result<Box<dyn Backend>, Missing> = match id {
            BackendId::Ollama { model } => {
                let url = &config.ollama_url.value;
                ollama::check(url, model)
                    .map(|info| Box::new(Ollama::new(url, model, config.num_ctx(), &info)) as _)
            }
            BackendId::Claude { model } => {
                // One binary serves every claude model: look for it once.
                if claude_bin.is_none() {
                    claude_bin = Some(locate_bin(claude::check, &config.claude_bin.value)?);
                }
                let bin = claude_bin.clone().expect("located above");
                bin.map(|bin| Box::new(Claude::new(bin, model.clone())) as _)
            }
            BackendId::Codex { model } => {
                if codex_bin.is_none() {
                    codex_bin = Some(locate_bin(codex::check, &config.codex_bin.value)?);
                }
                let bin = codex_bin.clone().expect("located above");
                bin.map(|bin| Box::new(Codex::new(bin, model.clone())) as _)
            }
            BackendId::Endpoint { name, model } => {
                endpoint::connect(name, config.endpoint(name)?, model)
                    .map(|backend| Box::new(backend) as _)
                    .map_err(Problem::into_missing)
            }
        };
        match backend {
            Ok(backend) => {
                ready.insert(id.to_string(), backend);
            }
            Err(m) if missing.contains(&m) => {}
            Err(m) => missing.push(m),
        }
    }
    match toolchain {
        Some(toolchain) if missing.is_empty() => Ok(Ready {
            toolchain,
            vad_model,
            backends: ready,
        }),
        _ => Err(Error::MissingTools(missing)),
    }
}

/// A backend's binary as an absolute path: the backend runs it from a temporary directory,
/// where a relative path would not resolve.
fn locate_bin(
    check: fn(&str) -> std::result::Result<ToolInfo, Missing>,
    bin: &str,
) -> Result<std::result::Result<PathBuf, Missing>> {
    let info = match check(bin) {
        Ok(info) => info,
        Err(m) => return Ok(Err(m)),
    };
    if info.path.is_absolute() {
        return Ok(Ok(info.path));
    }
    fs::canonicalize(&info.path).map(Ok).map_err(|e| {
        Error::Io(io::Error::new(
            e.kind(),
            format!("{}: {e}", info.path.display()),
        ))
    })
}

/// The status of the default backend for `doctor`: `ok`, `not reachable: …`,
/// `model missing: …` or `invalid: …`.
pub fn llm_status(config: &Config) -> String {
    let id = match config.llm_default.value.parse::<BackendId>() {
        Ok(id) => id,
        Err(e) => return invalid_status(e),
    };
    match &id {
        BackendId::Ollama { model } => match ollama::check(&config.ollama_url.value, model) {
            Ok(_) => "ok".to_string(),
            // Only a model absent from /api/tags is missing; any other failure, such as
            // /api/show failing, means the model cannot be reached.
            Err(m) if m.hint == format!("ollama pull {model}") => {
                format!("model missing: {}", m.hint)
            }
            Err(m) => format!("not reachable: {}", m.hint),
        },
        BackendId::Claude { .. } => match claude::check(&config.claude_bin.value) {
            Ok(_) => "ok".to_string(),
            Err(m) => format!("not reachable: {}", m.hint),
        },
        BackendId::Codex { .. } => match codex::check(&config.codex_bin.value) {
            Ok(_) => "ok".to_string(),
            Err(m) => format!("not reachable: {}", m.hint),
        },
        BackendId::Endpoint { name, model } => match config.endpoint(name) {
            Ok(found) => match endpoint::connect(name, found, model) {
                Ok(_) => "ok".to_string(),
                Err(Problem::ModelMissing(m)) => format!("model missing: {}", m.hint),
                Err(Problem::Unreachable(m)) => format!("not reachable: {}", m.hint),
            },
            Err(e) => invalid_status(e),
        },
    }
}

fn invalid_status(error: Error) -> String {
    match error {
        Error::Usage(message) => format!("invalid: {message}"),
        other => format!("invalid: {other}"),
    }
}

/// What `doctor` shows for `config`.
pub fn inspect(config: &Config) -> Result<Report> {
    let mut report = tools::inspect(&tool_paths(config)?, true);
    locate_vad_model(config, &mut report.missing)?;
    report.missing = with_pull_hint(report.missing, config);
    Ok(report)
}

/// Maps the configured binaries and model onto [`ToolPaths`]. A model name from the catalog
/// resolves into the models dir; a value that is neither a file nor a catalog name is kept
/// as a path, so preflight reports it as missing.
pub fn tool_paths(config: &Config) -> Result<ToolPaths> {
    let model = config
        .whisper_model
        .as_ref()
        .map(|setting| model_path(&setting.value, ModelKind::Whisper, config))
        .transpose()?;
    Ok(ToolPaths {
        ffmpeg: bin_path(&config.ffmpeg_bin.value, "ffmpeg"),
        ffprobe: bin_path(&config.ffprobe_bin.value, "ffprobe"),
        whisper: bin_path(&config.whisper_bin.value, "whisper-cli"),
        model,
        path: None,
    })
}

/// A configured binary. The tool's own name is left to the `PATH` lookup in `tools`, another
/// bare name is looked up on `PATH` here, and a value with a directory part is used as is.
fn bin_path(value: &str, tool: &str) -> Option<PathBuf> {
    if value == tool {
        return None;
    }
    let path = Path::new(value);
    if path.components().count() > 1 {
        return Some(path.to_path_buf());
    }
    let found = std::env::var_os("PATH").and_then(|search| {
        std::env::split_paths(&search)
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join(value))
            .find(|candidate| candidate.is_file())
    });
    Some(found.unwrap_or_else(|| path.to_path_buf()))
}

/// The config key that takes a model of `kind`.
fn model_key(kind: ModelKind) -> &'static str {
    match kind {
        ModelKind::Whisper => "whisper.model",
        ModelKind::Vad => "whisper.vad_model",
    }
}

/// A configured model of `kind`: a catalog name resolves into the models dir, anything else is
/// kept as a path. A catalog name of the other kind is `Error::Usage`.
fn model_path(value: &str, kind: ModelKind, config: &Config) -> Result<PathBuf> {
    match models::CATALOG.iter().find(|m| m.name == value) {
        Some(spec) if spec.kind != kind => {
            let what = match spec.kind {
                ModelKind::Whisper => "a whisper model",
                ModelKind::Vad => "a VAD model",
            };
            Err(Error::Usage(format!(
                "`{value}` is {what}; set it as {}, not {}",
                model_key(spec.kind),
                model_key(kind)
            )))
        }
        Some(_) => models::resolve(value, &config.models_dir_path()?),
        None => Ok(PathBuf::from(value)),
    }
}

/// The configured `whisper.vad_model` as a path, resolved like `whisper.model`.
fn vad_model_path(config: &Config) -> Result<Option<PathBuf>> {
    let Some(setting) = config.whisper_vad_model.as_ref() else {
        return Ok(None);
    };
    model_path(&setting.value, ModelKind::Vad, config).map(Some)
}

/// The name of a missing VAD model in [`Missing`].
const VAD_MODEL: &str = "vad model";

/// The configured VAD model, if it is a file. One that is configured but not a file is added
/// to `missing`; a metadata error other than not found is returned, naming the path.
fn locate_vad_model(config: &Config, missing: &mut Vec<Missing>) -> Result<Option<PathBuf>> {
    let Some(path) = vad_model_path(config)? else {
        return Ok(None);
    };
    match fs::metadata(&path) {
        Ok(meta) if meta.is_file() => return Ok(Some(path)),
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(Error::Io(io::Error::new(
                e.kind(),
                format!("{}: {e}", path.display()),
            )))
        }
    }
    missing.push(Missing {
        name: VAD_MODEL.to_string(),
        hint: format!("configured path {} is not a file", path.display()),
    });
    Ok(None)
}

/// A missing model or VAD model that is a catalog name only needs `minuta models pull <name>`.
fn with_pull_hint(mut missing: Vec<Missing>, config: &Config) -> Vec<Missing> {
    let settings = [
        ("model", &config.whisper_model),
        (VAD_MODEL, &config.whisper_vad_model),
    ];
    for (name, setting) in settings {
        let Some(setting) = setting else {
            continue;
        };
        if models::CATALOG.iter().any(|m| m.name == setting.value) {
            for item in missing.iter_mut().filter(|m| m.name == name) {
                item.hint = format!("run `minuta models pull {}`", setting.value);
            }
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::test_support::config;

    #[test]
    fn default_binaries_are_looked_up_by_tools() {
        let paths = tool_paths(&config(&[])).unwrap();
        assert_eq!(paths.ffmpeg, None);
        assert_eq!(paths.ffprobe, None);
        assert_eq!(paths.whisper, None);
        assert_eq!(paths.model, None);
        assert_eq!(paths.path, None);
    }

    #[test]
    fn configured_paths_are_used_as_is() {
        let paths = tool_paths(&config(&[
            ("MINUTA_FFMPEG_BIN", "/opt/ff/ffmpeg"),
            ("MINUTA_WHISPER_BIN", "./bin/whisper-cli"),
            ("MINUTA_FFPROBE_BIN", "no-such-tool-anywhere"),
        ]))
        .unwrap();
        assert_eq!(paths.ffmpeg, Some(PathBuf::from("/opt/ff/ffmpeg")));
        assert_eq!(paths.whisper, Some(PathBuf::from("./bin/whisper-cli")));
        // Not on PATH: kept, so preflight names it as the configured path.
        assert_eq!(paths.ffprobe, Some(PathBuf::from("no-such-tool-anywhere")));
    }

    #[test]
    fn catalog_model_resolves_into_models_dir() {
        let paths = tool_paths(&config(&[("MINUTA_WHISPER_MODEL", "base")])).unwrap();
        assert_eq!(
            paths.model,
            Some(models::default_dir().unwrap().join("ggml-base.bin"))
        );
        let paths = tool_paths(&config(&[("MINUTA_WHISPER_MODEL", "/m/x.bin")])).unwrap();
        assert_eq!(paths.model, Some(PathBuf::from("/m/x.bin")));
    }

    #[test]
    fn pull_hint_only_for_catalog_names() {
        let missing = || {
            vec![Missing {
                name: "model".into(),
                hint: "configured path is not a file".into(),
            }]
        };
        let hinted = with_pull_hint(missing(), &config(&[("MINUTA_WHISPER_MODEL", "small")]));
        assert_eq!(hinted[0].hint, "run `minuta models pull small`");
        let kept = with_pull_hint(missing(), &config(&[("MINUTA_WHISPER_MODEL", "/m/x.bin")]));
        assert_eq!(kept, missing());
    }

    #[test]
    fn vad_model_resolves_into_configured_models_dir() {
        let path = vad_model_path(&config(&[
            ("MINUTA_MODELS_DIR", "/m"),
            ("MINUTA_WHISPER_VAD_MODEL", "silero-v5.1.2"),
        ]));
        assert_eq!(
            path.unwrap(),
            Some(PathBuf::from("/m/vad/ggml-silero-v5.1.2.bin"))
        );
    }

    #[test]
    fn whisper_model_resolves_into_configured_models_dir() {
        let paths = tool_paths(&config(&[
            ("MINUTA_MODELS_DIR", "/m"),
            ("MINUTA_WHISPER_MODEL", "tiny"),
        ]));
        assert_eq!(
            paths.unwrap().model,
            Some(PathBuf::from("/m/ggml-tiny.bin"))
        );
    }

    #[test]
    fn bad_models_dir_does_not_break_a_file_model() {
        let paths = tool_paths(&config(&[
            ("MINUTA_MODELS_DIR", "~bob/models"),
            ("MINUTA_WHISPER_MODEL", "/abs/x.bin"),
        ]));
        assert_eq!(paths.unwrap().model, Some(PathBuf::from("/abs/x.bin")));
    }

    #[test]
    fn vad_model_unset_is_none() {
        assert_eq!(vad_model_path(&config(&[])).unwrap(), None);
    }

    #[test]
    fn catalog_vad_model_resolves_into_models_dir() {
        let path = vad_model_path(&config(&[("MINUTA_WHISPER_VAD_MODEL", "silero-v5.1.2")]));
        assert_eq!(
            path.unwrap(),
            Some(
                models::default_dir()
                    .unwrap()
                    .join("vad")
                    .join("ggml-silero-v5.1.2.bin")
            )
        );
    }

    #[test]
    fn vad_model_path_is_kept() {
        let path = vad_model_path(&config(&[("MINUTA_WHISPER_VAD_MODEL", "/m/vad.bin")]));
        assert_eq!(path.unwrap(), Some(PathBuf::from("/m/vad.bin")));
    }

    #[test]
    fn pull_hint_for_each_model_uses_its_own_setting() {
        let missing = || {
            vec![
                Missing {
                    name: "model".into(),
                    hint: "configured path is not a file".into(),
                },
                Missing {
                    name: "vad model".into(),
                    hint: "configured path is not a file".into(),
                },
            ]
        };
        let both = config(&[
            ("MINUTA_WHISPER_MODEL", "small"),
            ("MINUTA_WHISPER_VAD_MODEL", "silero-v5.1.2"),
        ]);
        let hinted = with_pull_hint(missing(), &both);
        assert_eq!(hinted[0].hint, "run `minuta models pull small`");
        assert_eq!(hinted[1].hint, "run `minuta models pull silero-v5.1.2`");
        let vad_only = config(&[
            ("MINUTA_WHISPER_MODEL", "/m/x.bin"),
            ("MINUTA_WHISPER_VAD_MODEL", "silero-v5.1.2"),
        ]);
        let hinted = with_pull_hint(missing(), &vad_only);
        assert_eq!(hinted[0].hint, "configured path is not a file");
        assert_eq!(hinted[1].hint, "run `minuta models pull silero-v5.1.2`");
    }

    #[test]
    fn preflight_without_templates_checks_no_backend() {
        // Missing media tools, but no backend: only the tools are reported.
        let cfg = config(&[
            ("MINUTA_FFMPEG_BIN", "/nonexistent/ffmpeg"),
            ("MINUTA_OLLAMA_URL", "http://127.0.0.1:9"),
        ]);
        let Err(Error::MissingTools(missing)) = preflight(&cfg, &[]) else {
            panic!("expected missing tools");
        };
        assert!(missing.iter().all(|m| !m.name.starts_with("ollama")));
    }
}
