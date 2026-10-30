//! The `minuta templates pull` command: choose templates from the catalog, show what will
//! happen, ask, download and report. All input and output goes through `Terminal`, so tests
//! drive it without a real terminal. Everything printed goes to stderr except the paths of
//! the files written, which go to stdout one per line.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use super::pull::Origin;
use crate::error::Result;

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

pub fn run(_origin: &Origin, _options: &Options<'_>, _term: &mut Terminal<'_>) -> Result<()> {
    unimplemented!()
}

/// Reads an answer to the numbered list: numbers (1-based, separated by commas or spaces),
/// `a` or `all`, or nothing. Returns 0-based indexes in the order given, without repeats.
pub fn parse_choice(_line: &str, _count: usize) -> Result<Vec<usize>> {
    unimplemented!()
}
