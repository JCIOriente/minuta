//! Command-line arguments and the commands. Only `run` and `transcribe` print to stdout
//! the job folder; `doctor`, `models list`, `models pull` and `config` print their data there.
//! Everything else goes to stderr.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};

use crate::config::{self, Config, Overrides, Source};
use crate::error::{Error, Result};
use crate::job::{Force, StepName};
use crate::models::{self, Pulled};
use crate::progress::{Mode, Progress};
use crate::runner;
use crate::tools::ToolInfo;

#[derive(Debug, Parser)]
#[command(name = "minuta", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Transcribe recordings into a job folder and print its path
    Run(RunArgs),
    /// Same as `run`
    Transcribe(RunArgs),
    /// Report tools, versions and the model; exit 1 if anything is missing
    Doctor,
    /// List or download whisper models
    #[command(subcommand)]
    Models(ModelsCommand),
    /// Show the config file path or the merged settings
    #[command(subcommand)]
    Config(ConfigCommand),
}

impl Command {
    /// Whether the command runs the pipeline, which handles Ctrl-C itself.
    pub fn runs_pipeline(&self) -> bool {
        matches!(self, Command::Run(_) | Command::Transcribe(_))
    }
}

#[derive(Debug, Subcommand)]
pub enum ModelsCommand {
    /// Show every catalog model and whether it is installed
    List,
    /// Download a model and verify its checksum
    Pull {
        /// tiny, base, small, medium or large-v3
        name: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print the config file path
    Path,
    /// Print every setting with its value and where it came from
    Show,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Recordings, joined in the order given
    #[arg(required = true)]
    pub files: Vec<PathBuf>,
    /// Job name (default: the first file's name without its media extension)
    #[arg(short, long)]
    pub name: Option<String>,
    /// Output directory for job folders
    #[arg(long, value_name = "DIR")]
    pub out: Option<String>,
    /// Speech language: auto or a whisper language code
    #[arg(long, value_name = "auto|xx")]
    pub lang: Option<String>,
    /// Model file or catalog name
    #[arg(long, value_name = "PATH|NAME")]
    pub model: Option<String>,
    /// Rerun every step, or STEP and every later one (put it after FILES)
    #[arg(long, value_name = "STEP", num_args = 0..=1, value_parser = parse_step)]
    pub force: Option<Option<StepName>>,
    /// Print only warnings and errors
    #[arg(short, long)]
    pub quiet: bool,
}

fn parse_step(s: &str) -> std::result::Result<StepName, String> {
    s.parse().map_err(|e| match e {
        Error::Usage(msg) => msg,
        other => other.to_string(),
    })
}

/// Runs `cli`. `interrupt` is the flag the Ctrl-C handler sets.
pub fn execute(cli: Cli, interrupt: &Arc<AtomicBool>) -> Result<()> {
    match cli.command {
        Command::Run(args) | Command::Transcribe(args) => run(args, interrupt),
        Command::Doctor => doctor(),
        Command::Models(ModelsCommand::List) => models_list(),
        Command::Models(ModelsCommand::Pull { name }) => models_pull(&name),
        Command::Config(ConfigCommand::Path) => config_path(),
        Command::Config(ConfigCommand::Show) => config_show(),
    }
}

/// The message for a failed command, for stderr. Missing tools get one line each with a hint.
pub fn render_error(err: &Error) -> String {
    let mut text = format!("minuta: {err}");
    if let Error::MissingTools(missing) = err {
        for item in missing {
            text.push_str(&format!("\n  {}: {}", item.name, item.hint));
        }
    }
    text
}

fn run(args: RunArgs, interrupt: &Arc<AtomicBool>) -> Result<()> {
    let config = Config::load(Overrides {
        out_dir: args.out,
        whisper_lang: args.lang,
        whisper_model: args.model,
        ..Overrides::default()
    })?;
    let options = runner::Options {
        files: args.files,
        name: args.name,
        force: args.force.map(|step| step.map_or(Force::All, Force::From)),
    };
    let mut progress = Progress::new(Mode::detect(args.quiet));
    let dir = runner::run(&options, &config, interrupt, &mut progress)?;
    println!("{}", dir.display());
    Ok(())
}

fn doctor() -> Result<()> {
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
    if report.missing.is_empty() {
        Ok(())
    } else {
        Err(Error::MissingTools(report.missing))
    }
}

fn models_list() -> Result<()> {
    for model in models::list(&models::default_dir()?) {
        let state = if model.installed {
            "installed"
        } else {
            "available"
        };
        println!("{:<10} {state:<10} {}", model.name, human_size(model.size));
    }
    Ok(())
}

fn models_pull(name: &str) -> Result<()> {
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
    let dir = models::default_dir()?;
    eprintln!(
        "pulling {name} ({}) into {}",
        human_size(spec.size),
        dir.display()
    );
    match models::pull(spec, &dir, models::DEFAULT_BASE_URL)? {
        Pulled::AlreadyPresent => eprintln!("{name} is already present"),
        Pulled::Downloaded => eprintln!("{name} downloaded and verified"),
    }
    println!("{}", models::resolve(name, &dir)?.display());
    Ok(())
}

/// Resolves the path without reading the file, so it works when the file is malformed.
fn config_path() -> Result<()> {
    let env = |key: &str| std::env::var(key).ok();
    let path = config::resolve_path(&env, config::default_path().as_deref())
        .ok_or_else(|| Error::Config("cannot determine the config file path".into()))?;
    println!("{}", path.display());
    Ok(())
}

fn config_show() -> Result<()> {
    let config = Config::load(Overrides::default())?;
    for (key, setting) in config.entries() {
        match setting {
            Some(s) => println!("{key} = {} ({})", s.value, source_name(s.source)),
            None => println!("{key} = (unset)"),
        }
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

/// `141.1 MiB`, `2.9 GiB`.
fn human_size(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    let mib = bytes as f64 / MIB;
    if mib < 1024.0 {
        format!("{mib:.1} MiB")
    } else {
        format!("{:.1} GiB", mib / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Missing;

    #[test]
    fn sizes() {
        assert_eq!(human_size(147_951_465), "141.1 MiB");
        assert_eq!(human_size(3_095_033_483), "2.9 GiB");
    }

    #[test]
    fn missing_tools_get_a_line_each() {
        let err = Error::MissingTools(vec![
            Missing {
                name: "ffmpeg".into(),
                hint: "brew install ffmpeg".into(),
            },
            Missing {
                name: "model".into(),
                hint: "run `minuta models pull base`".into(),
            },
        ]);
        assert_eq!(
            render_error(&err),
            "minuta: missing required tools: ffmpeg, model\n  \
             ffmpeg: brew install ffmpeg\n  \
             model: run `minuta models pull base`"
        );
    }

    #[test]
    fn force_takes_an_optional_step() {
        let force = |args: &[&str]| {
            let cli = Cli::try_parse_from(["minuta", "run", "a.mp4"].iter().chain(args)).unwrap();
            match cli.command {
                Command::Run(args) => args.force,
                other => panic!("parsed {other:?}"),
            }
        };
        assert_eq!(force(&[]), None);
        assert_eq!(force(&["--force"]), Some(None));
        assert_eq!(
            force(&["--force", "transcribe"]),
            Some(Some(StepName::Transcribe))
        );
        let err = Cli::try_parse_from(["minuta", "run", "a.mp4", "--force", "x"]).unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }
}
