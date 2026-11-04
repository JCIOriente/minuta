//! Command-line arguments and the commands. Only `run` and `transcribe` print to stdout
//! the job folder; `doctor`, `models list`, `models pull`, `config` and `templates` print
//! their data there. Everything else goes to stderr.

mod commands;
mod templates;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};

use crate::error::{Error, Result};
use crate::models;
use commands::{
    config_init, config_path, config_show, doctor, models_list, models_pull, run, Templates,
};
use templates::{templates_list, templates_pull, templates_show};

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
        /// Show neither the progress bar nor the start and finish lines
        #[arg(long)]
        quiet: bool,
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
    /// Write a commented template of every setting to the config file path and print the
    /// path; an existing file is left alone
    Init {
        /// Replace an existing config file
        #[arg(long)]
        force: bool,
    },
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
    /// Download curated templates from the repository (asks first)
    Pull(PullArgs),
}

#[derive(Debug, Args)]
pub struct PullArgs {
    /// Templates to download, by name (see --list)
    #[arg(value_name = "NAME", conflicts_with_all = ["all", "filter", "list"])]
    pub names: Vec<String>,
    /// Download every template in the catalog
    #[arg(long, conflicts_with_all = ["filter", "list"])]
    pub all: bool,
    /// Download the templates whose name, platform or tag contains TEXT
    #[arg(long, value_name = "TEXT", conflicts_with = "list")]
    pub filter: Option<String>,
    /// Show the catalog with the state of each template and download nothing
    #[arg(long, conflicts_with = "force")]
    pub list: bool,
    /// Folder to put the templates in [default: ./templates]
    #[arg(long, value_name = "DIR")]
    pub to: Option<PathBuf>,
    /// Replace templates you edited (the old file is saved as NAME.md.bak)
    #[arg(long)]
    pub force: bool,
    /// Do not ask for confirmation
    #[arg(long)]
    pub yes: bool,
    /// Branch, tag or commit of the repository to download from
    #[arg(long = "ref", value_name = "REF", default_value = "main")]
    pub reference: String,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[command(flatten)]
    pub pipeline: PipelineArgs,
    /// Generate a document with this template (repeatable)
    #[arg(short = 't', long = "template", value_name = "TEMPLATE")]
    pub templates: Vec<String>,
    /// Backend for every template: ollama:<model>, claude, claude:<model>, codex, codex:<model>,
    /// or <endpoint>:<model> for a server under [endpoints.<endpoint>] in the config
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
        Command::Models(ModelsCommand::Pull { name, quiet }) => models_pull(&name, quiet),
        Command::Config(ConfigCommand::Path) => config_path(),
        Command::Config(ConfigCommand::Show) => config_show(),
        Command::Config(ConfigCommand::Init { force }) => config_init(force),
        Command::Templates(TemplatesCommand::List) => templates_list(),
        Command::Templates(TemplatesCommand::Show { name }) => templates_show(&name),
        Command::Templates(TemplatesCommand::Pull(args)) => templates_pull(args),
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

/// Template files can come from a cloned repository: every control character (newlines,
/// carriage returns, escape sequences) becomes `?`, so a listing entry stays one line and
/// cannot drive the terminal.
pub(crate) fn sanitize(text: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Missing;
    use crate::progress::human_size;

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

    #[test]
    fn pull_ref_defaults_to_main() {
        let cli = Cli::try_parse_from(["minuta", "templates", "pull", "alpha"]).unwrap();
        match cli.command {
            Command::Templates(TemplatesCommand::Pull(args)) => {
                assert_eq!(args.reference, "main")
            }
            other => panic!("expected templates pull, got {other:?}"),
        }
    }
}
