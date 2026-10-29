//! Command-line arguments and the commands. Only `run` and `transcribe` print to stdout
//! the job folder; `doctor`, `models list`, `models pull`, `config` and `templates` print
//! their data there. Everything else goes to stderr.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};

use crate::config::{self, Config, Overrides, Source};
use crate::error::{Error, Result};
use crate::models::{self, Pulled};
use crate::progress::{Mode, Progress};
use crate::runner::{self, ForceArg};
use crate::template::discover::{discover, Dirs};
use crate::template::{Input, Layer, Output};
use crate::tools::ToolInfo;

#[derive(Debug, Parser)]
#[command(name = "minuta", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Transcribe recordings into a job folder, generate documents from templates and print
    /// the folder's path
    Run(RunArgs),
    /// Same as `run` without templates
    Transcribe(PipelineArgs),
    /// Report tools, versions and the model; exit 1 if anything is missing
    Doctor,
    /// List or download whisper models
    #[command(subcommand)]
    Models(ModelsCommand),
    /// Show the config file path or the merged settings
    #[command(subcommand)]
    Config(ConfigCommand),
    /// List the available templates or show one
    #[command(subcommand)]
    Templates(TemplatesCommand),
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
        #[arg(help = pull_name_help())]
        name: String,
    },
}

/// Lists every catalog model, so the help can't fall behind the catalog.
fn pull_name_help() -> String {
    let names: Vec<String> = models::CATALOG
        .iter()
        .map(|m| match m.kind {
            models::ModelKind::Whisper => m.name.to_string(),
            models::ModelKind::Vad => format!("{} (voice activity detection)", m.name),
        })
        .collect();
    match names.split_last() {
        Some((last, rest)) => format!("Model name: {}, or {last}", rest.join(", ")),
        None => "Model name".into(),
    }
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print the config file path
    Path,
    /// Print every setting with its value and where it came from
    Show,
}

#[derive(Debug, Subcommand)]
pub enum TemplatesCommand {
    /// Show every template with its layer, input and backend, then the invalid ones
    List,
    /// Print the file that defines a template (its path goes to stderr)
    Show {
        /// Template name
        name: String,
    },
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[command(flatten)]
    pub pipeline: PipelineArgs,
    /// Generate a document with this template (repeatable)
    #[arg(short = 't', long = "template", value_name = "TEMPLATE")]
    pub templates: Vec<String>,
    /// Backend for every template: ollama:<model>, claude or claude:<model>
    #[arg(long, value_name = "ID")]
    pub llm: Option<String>,
    /// Language the documents are written in (default: the detected speech language)
    #[arg(long, value_name = "CODE")]
    pub output_lang: Option<String>,
}

#[derive(Debug, Args)]
pub struct PipelineArgs {
    /// Recordings, joined in the order given
    #[arg(required = true)]
    pub files: Vec<PathBuf>,
    /// Job name (default: the first file's name without its media extension); picks another
    /// job, never replaces one
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
    /// Rerun every step and template, STEP and every later step plus every template, or
    /// TEMPLATE and the templates that depend on it; also replaces a job made from other
    /// sources (put it after FILES)
    #[arg(long, value_name = "STEP|TEMPLATE", num_args = 0..=1)]
    pub force: Option<Option<String>>,
    /// Print only warnings and errors
    #[arg(short, long)]
    pub quiet: bool,
}

/// Runs `cli`. `interrupt` is the flag the Ctrl-C handler sets.
pub fn execute(cli: Cli, interrupt: &Arc<AtomicBool>) -> Result<()> {
    match cli.command {
        Command::Run(args) => run(
            args.pipeline,
            Templates {
                names: args.templates,
                llm: args.llm,
                output_lang: args.output_lang,
            },
            interrupt,
        ),
        Command::Transcribe(args) => run(args, Templates::default(), interrupt),
        Command::Doctor => doctor(),
        Command::Models(ModelsCommand::List) => models_list(),
        Command::Models(ModelsCommand::Pull { name }) => models_pull(&name),
        Command::Config(ConfigCommand::Path) => config_path(),
        Command::Config(ConfigCommand::Show) => config_show(),
        Command::Templates(TemplatesCommand::List) => templates_list(),
        Command::Templates(TemplatesCommand::Show { name }) => templates_show(&name),
    }
}

/// The message for a failed command, for stderr. Missing tools get one line each with a hint.
/// Messages can quote template files from a cloned repository, so every control character
/// except the line breaks of this layout is neutralized.
pub fn render_error(err: &Error) -> String {
    let mut text = format!("minuta: {err}");
    if let Error::MissingTools(missing) = err {
        for item in missing {
            text.push_str(&format!("\n  {}: {}", item.name, item.hint));
        }
    }
    text.split('\n')
        .map(sanitize)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The template flags of `run`; `transcribe` has none.
#[derive(Debug, Default)]
struct Templates {
    names: Vec<String>,
    llm: Option<String>,
    output_lang: Option<String>,
}

fn run(args: PipelineArgs, templates: Templates, interrupt: &Arc<AtomicBool>) -> Result<()> {
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

fn models_list() -> Result<()> {
    for model in models::list(&models::default_dir()?)? {
        let state = if model.installed {
            "installed"
        } else {
            "available"
        };
        println!("{:<14} {state:<10} {}", model.name, human_size(model.size));
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
    match models::pull(spec, &dir)? {
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

fn templates_list() -> Result<()> {
    let catalog = discover(&Dirs::standard()?)?;
    let mut rows = vec![[
        "NAME".to_string(),
        "LAYER".to_string(),
        "INPUT".to_string(),
        "OUTPUT".to_string(),
        "LLM".to_string(),
        "DESCRIPTION".to_string(),
    ]];
    for t in catalog.templates.values() {
        rows.push([
            sanitize(&t.name),
            layer_name(t.layer).to_string(),
            match &t.input {
                Input::Transcript => "transcript".to_string(),
                Input::Template(dep) => sanitize(dep),
            },
            match &t.output {
                Output::Markdown => "markdown",
                Output::Fields(_) => "fields",
                Output::Chapters(_) => "chapters",
            }
            .to_string(),
            t.llm
                .as_ref()
                .map_or("(default)".to_string(), |id| sanitize(&id.to_string())),
            sanitize(t.description.as_deref().unwrap_or_default()),
        ]);
    }
    let width = |col: usize| {
        rows.iter()
            .map(|r| r[col].chars().count())
            .max()
            .unwrap_or(0)
    };
    let widths = [width(0), width(1), width(2), width(3), width(4)];
    for row in &rows {
        let line = format!(
            "{:<w0$}  {:<w1$}  {:<w2$}  {:<w3$}  {:<w4$}  {}",
            row[0],
            row[1],
            row[2],
            row[3],
            row[4],
            row[5],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2],
            w3 = widths[3],
            w4 = widths[4],
        );
        println!("{}", line.trim_end());
    }
    for invalid in &catalog.invalid {
        println!(
            "invalid  {}  {}",
            sanitize(&invalid.path.display().to_string()),
            sanitize(&single_line(&invalid.error))
        );
    }
    Ok(())
}

/// Prints the winning file of template `name`: its path (or `built-in`) to stderr, its
/// content to stdout.
fn templates_show(name: &str) -> Result<()> {
    let catalog = discover(&Dirs::standard()?)?;
    let template = catalog.get(name)?;
    match &template.path {
        Some(path) => eprintln!("{}", path.display()),
        None => eprintln!("built-in"),
    }
    print!("{}", template.raw);
    Ok(())
}

fn layer_name(layer: Layer) -> &'static str {
    match layer {
        Layer::BuiltIn => "built-in",
        Layer::User => "user",
        Layer::Project => "project",
    }
}

/// Template files can come from a cloned repository: every control character (newlines,
/// carriage returns, escape sequences) becomes `?`, so a listing entry stays one line and
/// cannot drive the terminal.
fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// An error message on one line, for listings.
fn single_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
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
    fn models_pull_help_lists_every_catalog_model() {
        use clap::CommandFactory;
        let mut cmd = Cli::command();
        let pull = cmd
            .find_subcommand_mut("models")
            .unwrap()
            .find_subcommand_mut("pull")
            .unwrap();
        let help = pull.render_help().to_string();
        for m in models::CATALOG {
            assert!(help.contains(m.name), "{} missing from:\n{help}", m.name);
        }
        assert!(
            help.contains(
                "Model name: tiny, base, small, medium, large-v3, \
                 or silero-v5.1.2 (voice activity detection)"
            ),
            "help was:\n{help}"
        );
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

    fn run_args(args: &[&str]) -> RunArgs {
        let cli = Cli::try_parse_from(["minuta", "run", "a.mp4"].iter().chain(args)).unwrap();
        match cli.command {
            Command::Run(args) => args,
            other => panic!("parsed {other:?}"),
        }
    }

    /// Step and template names are told apart by the runner, once templates are resolved.
    #[test]
    fn force_takes_an_optional_name() {
        let force = |args: &[&str]| run_args(args).pipeline.force;
        assert_eq!(force(&[]), None);
        assert_eq!(force(&["--force"]), Some(None));
        assert_eq!(
            force(&["--force", "transcribe"]),
            Some(Some("transcribe".to_string()))
        );
        assert_eq!(
            force(&["--force", "summary"]),
            Some(Some("summary".to_string()))
        );
    }

    #[test]
    fn template_flags() {
        let args = run_args(&["-t", "a", "--template", "b", "--llm", "claude"]);
        assert_eq!(args.templates, ["a", "b"]);
        assert_eq!(args.llm.as_deref(), Some("claude"));
        assert_eq!(args.output_lang, None);
        let args = run_args(&["--output-lang", "en"]);
        assert_eq!(args.output_lang.as_deref(), Some("en"));
        assert!(args.templates.is_empty());
    }

    #[test]
    fn transcribe_has_no_template_flags() {
        for flag in [
            &["-t", "a"][..],
            &["--llm", "claude"],
            &["--output-lang", "en"],
        ] {
            let argv = ["minuta", "transcribe", "a.mp4"].iter().chain(flag);
            let err = Cli::try_parse_from(argv).unwrap_err();
            assert_eq!(err.exit_code(), 2, "{flag:?}");
        }
    }

    #[test]
    fn render_error_neutralizes_control_characters_but_keeps_lines() {
        let err = Error::Usage("bad 'x\u{1b}[31m\u{7}'\nnext".into());
        assert_eq!(
            render_error(&err),
            "minuta: usage error: bad 'x?[31m?'\nnext"
        );
        let err = Error::MissingTools(vec![Missing {
            name: "ollama".into(),
            hint: "start \u{1b}]0;t\u{7}".into(),
        }]);
        assert_eq!(
            render_error(&err),
            "minuta: missing required tools: ollama\n  ollama: start ?]0;t?"
        );
    }

    #[test]
    fn sanitize_replaces_control_characters() {
        assert_eq!(sanitize("a\u{1b}[2K\rb\nc\td"), "a?[2K?b?c?d");
        assert_eq!(sanitize("acta · ñ"), "acta · ñ");
    }

    #[test]
    fn single_line_joins_lines() {
        assert_eq!(single_line("a\n  b\n\n"), "a; b");
    }
}
