//! The `minuta templates pull` command: choose templates from the catalog, show what will
//! happen, ask, download and report. All input and output goes through `Terminal`, so tests
//! drive it without a real terminal. Everything printed goes to stderr except the paths of
//! the files written, which go to stdout one per line.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use super::catalog::{select, Entry, Selection, State};
use super::pull::{
    apply, fetch_catalog, fetch_file, plan, read_manifest, Done, Item, Origin, Outcome,
};
use crate::cli::sanitize;
use crate::error::{Error, Result};

/// Which templates to download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Names(Vec<String>),
    All,
    Filter(String),
    /// Nothing was given: open a numbered list (only on a terminal).
    Ask,
}

pub struct Options<'a> {
    pub choice: Choice,
    /// Print the catalog with each template's state and download nothing.
    pub list: bool,
    /// The folder the templates go to, absolute.
    pub dest: PathBuf,
    pub force: bool,
    /// Skip the confirmation.
    pub yes: bool,
    /// Names minuta can already use (built-in, user and project templates), to notice a
    /// template whose `input:` is missing.
    pub known: &'a [String],
}

pub struct Terminal<'a> {
    /// Whether a person can answer prompts (stdin and stderr are terminals).
    pub interactive: bool,
    pub input: &'a mut dyn BufRead,
    pub stdout: &'a mut dyn Write,
    pub stderr: &'a mut dyn Write,
}

pub fn run(origin: &Origin, options: &Options<'_>, term: &mut Terminal<'_>) -> Result<()> {
    // Refuse before any request when nobody can answer.
    if !options.list {
        if options.choice == Choice::Ask && !term.interactive {
            return Err(Error::Usage(
                "nothing selected: name the templates to download, or use --all or --filter TEXT (a list opens only on a terminal)"
                    .into(),
            ));
        }
        if !options.yes && !term.interactive {
            return Err(Error::Usage(
                "minuta asks before it downloads: pass --yes, or run it from a terminal".into(),
            ));
        }
    }

    let catalog = fetch_catalog(origin)?;
    let (mut manifest, warning) = read_manifest(&options.dest)?;
    if let Some(warning) = warning {
        // `Manifest::parse` already escaped the control characters in it.
        writeln!(term.stderr, "warning: {warning}")?;
    }

    if options.list {
        let all: Vec<&Entry> = catalog.entries.iter().collect();
        let items = plan(&options.dest, &all, &manifest)?;
        writeln!(term.stderr, "templates from {}", origin.url(""))?;
        write!(term.stderr, "{}", table(&items))?;
        return Ok(());
    }

    let chosen: Vec<&Entry> = match &options.choice {
        Choice::All => select(&catalog, Selection::All)?,
        Choice::Names(names) => select(&catalog, Selection::Names(names))?,
        Choice::Filter(text) => select(&catalog, Selection::Filter(text))?,
        Choice::Ask => {
            let all: Vec<&Entry> = catalog.entries.iter().collect();
            let items = plan(&options.dest, &all, &manifest)?;
            write!(term.stderr, "{}", numbered(&items))?;
            write!(
                term.stderr,
                "Select templates (numbers separated by commas or spaces, a for all, empty to cancel): "
            )?;
            term.stderr.flush()?;
            let mut line = String::new();
            term.input.read_line(&mut line)?;
            parse_choice(&line, all.len())?
                .into_iter()
                .map(|i| all[i])
                .collect()
        }
    };
    if chosen.is_empty() {
        writeln!(term.stderr, "nothing selected")?;
        return Ok(());
    }

    let items = plan(&options.dest, &chosen, &manifest)?;
    let width = items
        .iter()
        .map(|i| file_name(i).chars().count())
        .max()
        .unwrap_or(0);

    let writes = items.iter().any(|i| will_write(i, options.force));
    if writes && !options.yes {
        writeln!(term.stderr, "minuta will download from:")?;
        writeln!(term.stderr, "  {}", origin.url(""))?;
        for item in &items {
            writeln!(
                term.stderr,
                "  {:<width$}  ({})",
                file_name(item),
                prompt_note(item, options.force)
            )?;
        }
        writeln!(term.stderr, "into {}", options.dest.display())?;
        write!(term.stderr, "Continue? [y/N] ")?;
        term.stderr.flush()?;
        let mut answer = String::new();
        term.input.read_line(&mut answer)?;
        if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
            writeln!(term.stderr, "cancelled")?;
            return Ok(());
        }
    }

    let done = apply(
        &options.dest,
        &items,
        options.force,
        &mut |entry| fetch_file(origin, entry),
        &mut manifest,
    )?;
    for item in &done {
        writeln!(
            term.stderr,
            "{:<width$}  {}",
            format!("{}.md", item.name),
            result_text(item.outcome, &item.name)
        )?;
        if written(item.outcome) {
            writeln!(term.stdout, "{}", item.path.display())?;
        }
    }
    if !writes {
        writeln!(term.stderr, "nothing to download")?;
    }
    missing_inputs(&chosen, &done, options.known, term)?;
    Ok(())
}

fn file_name(item: &Item<'_>) -> String {
    format!("{}.md", item.entry.name)
}

/// Whether `apply` will download this item.
fn will_write(item: &Item<'_>, force: bool) -> bool {
    match item.state {
        State::New | State::UpdateAvailable => true,
        State::Modified => force,
        State::UpToDate => false,
    }
}

fn written(outcome: Outcome) -> bool {
    match outcome {
        Outcome::Created | Outcome::Updated | Outcome::Overwrote => true,
        Outcome::KeptUpToDate | Outcome::KeptModified => false,
    }
}

fn prompt_note(item: &Item<'_>, force: bool) -> String {
    match (item.state, force) {
        (State::New, _) => "new".to_string(),
        (State::UpdateAvailable, _) => "update available".to_string(),
        (State::Modified, true) => format!(
            "modified: replaced, old file saved as {}.bak",
            file_name(item)
        ),
        (State::Modified, false) => "modified: skipped, use --force to overwrite".to_string(),
        (State::UpToDate, _) => "up to date: skipped".to_string(),
    }
}

fn result_text(outcome: Outcome, name: &str) -> String {
    match outcome {
        Outcome::Created => "written".to_string(),
        Outcome::Updated => "updated".to_string(),
        Outcome::Overwrote => format!("overwritten (backup: {name}.md.bak)"),
        Outcome::KeptUpToDate => "up to date".to_string(),
        Outcome::KeptModified => "skipped (modified; use --force to overwrite)".to_string(),
    }
}

/// The catalog as columns: name, platform, state and description. The text of the catalog
/// comes from the network, so it is made safe to print.
fn table(items: &[Item<'_>]) -> String {
    let mut rows = vec![[
        "NAME".to_string(),
        "PLATFORM".to_string(),
        "STATE".to_string(),
        "DESCRIPTION".to_string(),
    ]];
    for item in items {
        rows.push([
            sanitize(&item.entry.name),
            sanitize(item.entry.platform.as_deref().unwrap_or("-")),
            item.state.label().to_string(),
            sanitize(&item.entry.description),
        ]);
    }
    let width = |col: usize| {
        rows.iter()
            .map(|r| r[col].chars().count())
            .max()
            .unwrap_or(0)
    };
    let (w0, w1, w2) = (width(0), width(1), width(2));
    let mut out = String::new();
    for row in &rows {
        let line = format!(
            "{:<w0$}  {:<w1$}  {:<w2$}  {}",
            row[0], row[1], row[2], row[3]
        );
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// The numbered list shown when no template was named: number, name, state, description.
fn numbered(items: &[Item<'_>]) -> String {
    let w0 = items
        .iter()
        .map(|i| i.entry.name.chars().count())
        .max()
        .unwrap_or(0);
    let w1 = items
        .iter()
        .map(|i| i.state.label().chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for (index, item) in items.iter().enumerate() {
        let line = format!(
            "{:>2}) {:<w0$}  {:<w1$}  {}",
            index + 1,
            sanitize(&item.entry.name),
            item.state.label(),
            sanitize(&item.entry.description)
        );
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// A written template whose `input:` chain names a template minuta cannot find still works
/// on its own files, but `minuta run` would fail on it: say so.
fn missing_inputs(
    chosen: &[&Entry],
    done: &[Done],
    known: &[String],
    term: &mut Terminal<'_>,
) -> Result<()> {
    for item in done.iter().filter(|d| written(d.outcome)) {
        let Some(entry) = chosen.iter().find(|e| e.name == item.name) else {
            continue;
        };
        let Some(chain) = &entry.input else { continue };
        for dep in chain.split("->").map(str::trim) {
            let available = dep.is_empty()
                || dep == "transcript"
                || known.iter().any(|k| k == dep)
                || chosen.iter().any(|e| e.name == dep);
            if !available {
                writeln!(
                    term.stderr,
                    "note: {} needs template {}, which is not available (pull it too or add it to your templates)",
                    sanitize(&entry.name),
                    sanitize(dep)
                )?;
            }
        }
    }
    Ok(())
}

/// Reads an answer to the numbered list: numbers (1-based, separated by commas or spaces),
/// `a` or `all`, or nothing. Returns 0-based indexes in the order given, without repeats.
pub fn parse_choice(line: &str, count: usize) -> Result<Vec<usize>> {
    let tokens: Vec<&str> = line
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .collect();
    if let [only] = tokens.as_slice() {
        if only.eq_ignore_ascii_case("a") || only.eq_ignore_ascii_case("all") {
            return Ok((0..count).collect());
        }
    }
    let mut chosen = Vec::new();
    for token in tokens {
        let number = Some(token)
            .filter(|t| t.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|t| t.parse::<usize>().ok())
            .filter(|n| (1..=count).contains(n));
        let Some(number) = number else {
            return Err(Error::Usage(format!(
                "invalid choice {token:?} (enter numbers from 1 to {count}, 'a' for all, or nothing to cancel)"
            )));
        };
        if !chosen.contains(&(number - 1)) {
            chosen.push(number - 1);
        }
    }
    Ok(chosen)
}
