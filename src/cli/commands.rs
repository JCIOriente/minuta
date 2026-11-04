//! The `run`, `transcribe`, `doctor`, `models` and `config` commands.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use super::PipelineArgs;
use crate::config::{self, Config, Overrides, Source};
use crate::config_init::{self, Written};
use crate::error::{Error, Result};
use crate::models;
use crate::progress::{human_size, Mode, Progress, PullReport};
use crate::runner::{self, ForceArg};
use crate::tools::ToolInfo;

/// The template flags of `run`; `transcribe` has none.
#[derive(Debug, Default)]
pub(super) struct Templates {
    pub(super) names: Vec<String>,
    pub(super) llm: Option<String>,
    pub(super) output_lang: Option<String>,
}

pub(super) fn run(
    args: PipelineArgs,
    templates: Templates,
    interrupt: &Arc<AtomicBool>,
) -> Result<()> {
    let config = Config::load(Overrides {
        out_dir: args.out,
        whisper_lang: args.lang,
        whisper_model: args.model,
        llm_default: templates.llm,
        llm_output_lang: templates.output_lang,
        ..Overrides::default()
    })?;
    let options = runner::Options {
        files: args.files,
        name: args.name,
        force: args
            .force
            .map(|name| name.map_or(ForceArg::All, ForceArg::Name)),
        templates: templates.names,
    };
    let mut progress = Progress::new(Mode::detect(args.quiet));
    let dir = runner::run(&options, &config, interrupt, &mut progress)?;
    println!("{}", dir.display());
    Ok(())
}

pub(super) fn doctor() -> Result<()> {
    let config = Config::load(Overrides::default())?;
    let report = runner::inspect(&config)?;
    let tool = |info: &Option<ToolInfo>| match info {
        Some(t) => format!("{} ({})", t.version, t.path.display()),
        None => "missing".to_string(),
    };
    println!("{:<12} {}", "ffmpeg", tool(&report.ffmpeg));
    println!("{:<12} {}", "ffprobe", tool(&report.ffprobe));
    println!("{:<12} {}", "whisper-cli", tool(&report.whisper));
    let model = match &report.model {
        Some(path) => path.display().to_string(),
        None => "missing".to_string(),
    };
    println!("{:<12} {model}", "model");
    println!(
        "{:<12} {} ({})",
        "llm",
        config.llm_default.value,
        runner::llm_status(&config)
    );
    if report.missing.is_empty() {
        Ok(())
    } else {
        Err(Error::MissingTools(report.missing))
    }
}

/// The models directory from the resolved config (env, then file, then `~/.minuta/models`).
fn models_dir() -> Result<PathBuf> {
    Config::load(Overrides::default())?.models_dir_path()
}

pub(super) fn models_list() -> Result<()> {
    let dir = models_dir()?;
    let rows = models::list(&dir)?;
    println!("models in {}", dir.display());
    for model in rows {
        let state = if model.installed {
            "installed"
        } else {
            "available"
        };
        println!("{:<14} {state:<10} {}", model.name, human_size(model.size));
    }
    Ok(())
}

pub(super) fn models_pull(name: &str, quiet: bool) -> Result<()> {
    let spec = models::CATALOG
        .iter()
        .find(|m| m.name == name)
        .ok_or_else(|| {
            let names: Vec<_> = models::CATALOG.iter().map(|m| m.name).collect();
            Error::Usage(format!(
                "unknown model `{name}`; expected one of {}",
                names.join(", ")
            ))
        })?;
    let dir = models_dir()?;
    let mut report = PullReport::new(if quiet {
        Mode::Quiet
    } else {
        Mode::detect(false)
    });
    report.start(name, spec.size, &dir);
    let outcome = models::pull_with_progress(spec, &dir, &mut |done| report.progress(done));
    match outcome {
        Ok(pulled) => report.finish(name, pulled),
        Err(e) => {
            report.abandon();
            return Err(e);
        }
    }
    println!("{}", models::resolve(name, &dir)?.display());
    Ok(())
}

/// Resolves the path without reading the file, so it works when the file is malformed.
pub(super) fn config_path() -> Result<()> {
    let env = |key: &str| std::env::var(key).ok();
    let path = config::resolve_path(&env, config::default_path().as_deref())
        .ok_or_else(|| Error::Config("cannot determine the config file path".into()))?;
    println!("{}", path.display());
    Ok(())
}

pub(super) fn config_show() -> Result<()> {
    let config = Config::load(Overrides::default())?;
    let fixed = config
        .entries()
        .into_iter()
        .map(|(key, setting)| (key.to_string(), setting));
    for (key, setting) in fixed.chain(config.endpoint_entries()) {
        match setting {
            Some(s) => println!("{key} = {} ({})", s.value, source_name(s.source)),
            None => println!("{key} = (unset)"),
        }
    }
    Ok(())
}

/// Writes the template without reading an existing file, so a malformed one does not block it.
pub(super) fn config_init(force: bool) -> Result<()> {
    let env = |key: &str| std::env::var(key).ok();
    let path = config::resolve_path(&env, config::default_path().as_deref())
        .ok_or_else(|| Error::Config("cannot determine the config file path".into()))?;
    match config_init::write(&path, force)? {
        Written::Created => println!("created {}", path.display()),
        Written::Overwrote => println!("overwrote {}", path.display()),
        Written::Exists => println!(
            "{} already exists; nothing changed (use --force to overwrite)",
            path.display()
        ),
    }
    Ok(())
}

fn source_name(source: Source) -> &'static str {
    match source {
        Source::Flag => "flag",
        Source::Env => "env",
        Source::File => "file",
        Source::Default => "default",
    }
}
