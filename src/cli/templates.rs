use std::io::IsTerminal;
use std::path::PathBuf;

use super::{sanitize, single_line, PullArgs};
use crate::error::{Error, Result};
use crate::progress::Mode;
use crate::template::discover::{discover, Dirs};
use crate::template::pull::Origin;
use crate::template::pull_cmd::{self, Choice};
use crate::template::{Input, Layer, Output};

pub(super) fn templates_list() -> Result<()> {
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
pub(super) fn templates_show(name: &str) -> Result<()> {
    let catalog = discover(&Dirs::standard()?)?;
    let template = catalog.get(name)?;
    match &template.path {
        Some(path) => eprintln!("{}", path.display()),
        None => eprintln!("built-in"),
    }
    print!("{}", template.raw);
    Ok(())
}

/// Where `templates pull` downloads from: `MINUTA_TEMPLATES_URL` (a folder URL; used by
/// tests, not documented) or the repository at `--ref`.
fn pull_origin(reference: &str, url_override: Option<std::ffi::OsString>) -> Result<Origin> {
    match url_override {
        Some(url) if !url.is_empty() => {
            let url = url.to_str().ok_or_else(|| {
                Error::Usage("MINUTA_TEMPLATES_URL is not valid UTF-8".to_string())
            })?;
            Origin::new(url)
        }
        _ => Origin::github(reference),
    }
}

/// The names of every template minuta can use: built-in, user and project.
fn known_templates() -> Result<Vec<String>> {
    Ok(discover(&Dirs::standard()?)?
        .templates
        .keys()
        .cloned()
        .collect())
}

pub(super) fn templates_pull(args: PullArgs) -> Result<()> {
    let origin = pull_origin(&args.reference, std::env::var_os("MINUTA_TEMPLATES_URL"))?;
    let cwd = std::env::current_dir().map_err(|e| {
        Error::Io(std::io::Error::new(
            e.kind(),
            format!("cannot determine the current directory: {e}"),
        ))
    })?;
    let dest = cwd.join(args.to.unwrap_or_else(|| PathBuf::from("templates")));
    // Only the note about a missing `input:` template needs to know what minuta can use: a
    // folder that cannot be read must not stop a download, so it is reported and the note
    // is skipped.
    let known = match known_templates() {
        Ok(names) => Some(names),
        Err(e) => {
            eprintln!(
                "warning: cannot check which templates minuta already has: {}; notes about missing input templates are skipped",
                sanitize(&e.to_string())
            );
            None
        }
    };
    let choice = if !args.names.is_empty() {
        Choice::Names(args.names)
    } else if args.all {
        Choice::All
    } else if let Some(text) = args.filter {
        Choice::Filter(text)
    } else {
        Choice::Ask
    };
    let options = pull_cmd::Options {
        choice,
        list: args.list,
        dest,
        force: args.force,
        yes: args.yes,
        known: known.as_deref(),
        mode: Mode::detect(false),
    };
    let stdin = std::io::stdin();
    let interactive = stdin.is_terminal() && std::io::stderr().is_terminal();
    pull_cmd::run(
        &origin,
        &options,
        &mut pull_cmd::Terminal {
            interactive,
            input: &mut stdin.lock(),
            // Not locked: the progress bar's ticker thread writes to stderr too, and a lock
            // held here for the whole run would block it while it holds the bar's state.
            stdout: &mut std::io::stdout(),
            stderr: &mut std::io::stderr(),
        },
    )
}

fn layer_name(layer: Layer) -> &'static str {
    match layer {
        Layer::BuiltIn => "built-in",
        Layer::User => "user",
        Layer::Project => "project",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_origin_downloads_from_the_repository_at_the_ref() {
        let origin = pull_origin("v1", None).unwrap();
        assert_eq!(
            origin.url("index.toml"),
            "https://raw.githubusercontent.com/JCIOriente/minuta/v1/templates/index.toml"
        );
    }

    #[test]
    fn pull_origin_url_variable_replaces_the_repository_and_the_ref() {
        let origin = pull_origin("v1", Some("https://h.example/t".into())).unwrap();
        assert_eq!(origin.url("a.md"), "https://h.example/t/a.md");
    }

    #[test]
    fn pull_origin_empty_url_variable_is_ignored() {
        let origin = pull_origin("v1", Some("".into())).unwrap();
        assert_eq!(
            origin.url("a.md"),
            "https://raw.githubusercontent.com/JCIOriente/minuta/v1/templates/a.md"
        );
    }

    #[test]
    fn pull_origin_rejects_a_bad_ref() {
        match pull_origin("a b", None) {
            Err(Error::Usage(m)) => assert_eq!(
                m,
                "invalid ref \"a b\" (use letters, digits, '.', '_', '-' and '/')"
            ),
            other => panic!("expected usage error, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn pull_origin_rejects_a_url_variable_that_is_not_utf8() {
        use std::os::unix::ffi::OsStringExt;
        let value = std::ffi::OsString::from_vec(vec![0xff, 0xfe]);
        match pull_origin("main", Some(value)) {
            Err(Error::Usage(m)) => assert_eq!(m, "MINUTA_TEMPLATES_URL is not valid UTF-8"),
            other => panic!("expected usage error, got {other:?}"),
        }
    }
}
