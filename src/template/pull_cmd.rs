//! The `minuta templates pull` command: choose templates from the catalog, show what will
//! happen, ask, download and report. All input and output goes through `Terminal`, so tests
//! drive it without a real terminal. Everything printed goes to stderr except the paths of
//! the files written, which go to stdout one per line.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use super::catalog::{select, Entry, Manifest, Selection};
use super::pull::{
    apply, fetch_catalog, fetch_file, outcome_for, plan, read_manifest, Done, Item, Origin, Outcome,
};
use super::valid_stem;
use crate::cli::sanitize;
use crate::error::{Error, Result};
use crate::progress::{Mode, TemplatesReport};

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
    /// template whose `input:` is missing. `None` when that could not be found out: no note
    /// is made then.
    pub known: Option<&'a [String]>,
    /// How much progress to show on stderr while files are fetched.
    pub mode: Mode,
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

    if catalog.entries.is_empty() && matches!(options.choice, Choice::All | Choice::Ask) {
        writeln!(term.stderr, "the catalog has no templates")?;
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
            let line = read_answer(term)?;
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
        writeln!(
            term.stderr,
            "into {}",
            sanitize(&options.dest.display().to_string())
        )?;
        write!(term.stderr, "Continue? [y/N] ")?;
        term.stderr.flush()?;
        let answer = read_answer(term)?;
        if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
            writeln!(term.stderr, "cancelled")?;
            return Ok(());
        }
    }

    let (applied, _) = apply_reported(
        &options.dest,
        &items,
        options.force,
        options.mode,
        &mut |entry| fetch_file(origin, entry),
        &mut manifest,
    );
    let done = applied?;
    for item in &done {
        writeln!(
            term.stderr,
            "{:<width$}  {}",
            md(&item.name),
            result_text(item.outcome, &item.name)
        )?;
        if written(item.outcome) {
            writeln!(term.stdout, "{}", item.path.display())?;
        }
    }
    if !writes {
        writeln!(term.stderr, "nothing to download")?;
    }
    missing_inputs(&chosen, &done, options, term)?;
    Ok(())
}

/// Reads one line the user typed. At the end of the input (Ctrl-D) nothing was typed and no
/// Enter moved the cursor, so the line is ended here.
fn read_answer(term: &mut Terminal<'_>) -> Result<String> {
    let mut line = String::new();
    if term.input.read_line(&mut line)? == 0 {
        writeln!(term.stderr)?;
    }
    Ok(line)
}

/// The file of template `name`.
fn md(name: &str) -> String {
    format!("{name}.md")
}

fn file_name(item: &Item<'_>) -> String {
    md(&item.entry.name)
}

/// Whether `apply` will download this item.
fn will_write(item: &Item<'_>, force: bool) -> bool {
    written(outcome_for(item.state, force))
}

fn written(outcome: Outcome) -> bool {
    match outcome {
        Outcome::Created | Outcome::Updated | Outcome::Overwrote => true,
        Outcome::KeptUpToDate | Outcome::KeptModified => false,
    }
}

/// What the prompt says will happen to a file: derived from the outcome `apply` will have.
fn prompt_note(item: &Item<'_>, force: bool) -> String {
    match outcome_for(item.state, force) {
        Outcome::Created => "new".to_string(),
        Outcome::Updated => "update available".to_string(),
        Outcome::Overwrote => format!(
            "modified: replaced, old file saved as {}.bak",
            file_name(item)
        ),
        Outcome::KeptModified => "modified: skipped, use --force to overwrite".to_string(),
        Outcome::KeptUpToDate => "up to date: skipped".to_string(),
    }
}

fn result_text(outcome: Outcome, name: &str) -> String {
    match outcome {
        Outcome::Created => "written".to_string(),
        Outcome::Updated => "updated".to_string(),
        Outcome::Overwrote => format!("overwritten (backup: {}.bak)", md(name)),
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
    options: &Options<'_>,
    term: &mut Terminal<'_>,
) -> Result<()> {
    let Some(known) = options.known else {
        return Ok(());
    };
    for item in done.iter().filter(|d| written(d.outcome)) {
        let Some(entry) = chosen.iter().find(|e| e.name == item.name) else {
            continue;
        };
        let Some(chain) = &entry.input else { continue };
        for dep in chain.split("->").map(str::trim) {
            let available = dep.is_empty()
                || dep == "transcript"
                || known.iter().any(|k| k == dep)
                || chosen.iter().any(|e| e.name == dep)
                || in_destination(&options.dest, dep, term)?;
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

/// Whether the destination folder already has a template called `name` (pulled earlier). When
/// that cannot be found out it counts as there, so no wrong note is made, and a warning says
/// why.
fn in_destination(dest: &Path, name: &str, term: &mut Terminal<'_>) -> Result<bool> {
    if !valid_stem(name) {
        return Ok(false);
    }
    let path = dest.join(md(name));
    match path.try_exists() {
        Ok(found) => Ok(found),
        Err(e) => {
            writeln!(
                term.stderr,
                "warning: cannot check {}: {}",
                sanitize(&path.display().to_string()),
                sanitize(&e.to_string())
            )?;
            Ok(true)
        }
    }
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

/// `apply` with a progress bar over the files that will be downloaded. The bar is gone when
/// this returns, whether it worked or not, so result lines and errors print cleanly. The
/// report comes back so tests can see what it was told.
fn apply_reported(
    dest: &Path,
    items: &[Item<'_>],
    force: bool,
    mode: Mode,
    fetch: &mut dyn FnMut(&Entry) -> Result<Vec<u8>>,
    manifest: &mut Manifest,
) -> (Result<Vec<Done>>, TemplatesReport) {
    let mut report = TemplatesReport::new(mode, download_count(items, force));
    let applied = apply(
        dest,
        items,
        force,
        &mut |entry| {
            report.fetching(&entry.name);
            let bytes = fetch(entry);
            report.fetched();
            bytes
        },
        manifest,
    );
    report.clear();
    (applied, report)
}

/// How many of `items` will be downloaded: kept and up-to-date files are not counted.
fn download_count(items: &[Item<'_>], force: bool) -> usize {
    items.iter().filter(|i| will_write(i, force)).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::catalog::{sha256_hex, State};

    fn entry(name: &str) -> Entry {
        Entry {
            name: name.to_string(),
            description: String::new(),
            platform: None,
            tags: Vec::new(),
            input: None,
            sha256: String::new(),
        }
    }

    #[test]
    fn bar_total_counts_only_files_to_download() {
        let entries: Vec<Entry> = ["a", "b", "c", "d"].map(entry).into();
        let states = [
            State::New,
            State::UpToDate,
            State::Modified,
            State::UpdateAvailable,
        ];
        let items: Vec<Item<'_>> = entries
            .iter()
            .zip(states)
            .map(|(entry, state)| Item {
                entry,
                path: PathBuf::from(md(&entry.name)),
                state,
            })
            .collect();
        // Modified is kept without --force; with it, it is replaced.
        assert_eq!(download_count(&items, false), 2);
        assert_eq!(download_count(&items, true), 3);
    }

    fn valid(name: &str) -> (Entry, Vec<u8>) {
        let text = format!("---\ndescription: {name}\n---\nWrite about {{{{job_name}}}}.\n");
        let mut e = entry(name);
        e.sha256 = sha256_hex(text.as_bytes());
        (e, text.into_bytes())
    }

    #[test]
    fn report_is_told_the_files_to_download_not_the_catalog_size() {
        let dest = tempfile::tempdir().unwrap();
        let (a, a_bytes) = valid("a");
        let (b, _) = valid("b");
        let items = [
            Item {
                entry: &a,
                path: dest.path().join("a.md"),
                state: State::New,
            },
            Item {
                entry: &b,
                path: dest.path().join("b.md"),
                state: State::UpToDate,
            },
        ];
        let (applied, report) = apply_reported(
            dest.path(),
            &items,
            false,
            Mode::Live,
            &mut |_| Ok(a_bytes.clone()),
            &mut Manifest::default(),
        );
        applied.unwrap();
        assert_eq!(report.total(), 1);
        assert!(!report.bar_active());
    }

    #[test]
    fn bar_cleared_on_fetch_failure() {
        let dest = tempfile::tempdir().unwrap();
        let (a, a_bytes) = valid("a");
        let (b, _) = valid("b");
        let items = [
            Item {
                entry: &a,
                path: dest.path().join("a.md"),
                state: State::New,
            },
            Item {
                entry: &b,
                path: dest.path().join("b.md"),
                state: State::New,
            },
        ];
        let mut calls = 0;
        let (applied, report) = apply_reported(
            dest.path(),
            &items,
            false,
            Mode::Live,
            &mut |_| {
                calls += 1;
                if calls == 1 {
                    Ok(a_bytes.clone())
                } else {
                    Err(Error::Usage("boom".into()))
                }
            },
            &mut Manifest::default(),
        );
        assert!(applied.is_err());
        assert_eq!(calls, 2, "the bar was drawn for the first file");
        assert!(!report.bar_active());
    }
}
