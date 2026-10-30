//! Downloading curated templates: the origin they come from, the plan of what to write, and
//! the verified, all-or-nothing write. No prompts and no printing: the CLI owns those.

use std::path::{Path, PathBuf};

use super::catalog::{Catalog, Entry, Manifest, State};
use crate::error::Result;

/// Largest `index.toml` accepted.
pub const CATALOG_CAP: u64 = 1024 * 1024;
/// Largest template file accepted.
pub const FILE_CAP: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    base: String,
}

impl Origin {
    pub fn new(_base: &str) -> Result<Origin> {
        unimplemented!()
    }

    pub fn github(_reference: &str) -> Result<Origin> {
        unimplemented!()
    }

    pub fn host(&self) -> &str {
        unimplemented!()
    }

    pub fn url(&self, _file: &str) -> String {
        unimplemented!()
    }
}

pub fn fetch_catalog(_origin: &Origin) -> Result<Catalog> {
    unimplemented!()
}

pub fn fetch_file(_origin: &Origin, _entry: &Entry) -> Result<Vec<u8>> {
    unimplemented!()
}

pub fn read_manifest(_dest: &Path) -> Result<(Manifest, Option<String>)> {
    unimplemented!()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item<'a> {
    pub entry: &'a Entry,
    pub path: PathBuf,
    pub state: State,
}

pub fn plan<'a>(
    _dest: &Path,
    _selection: &[&'a Entry],
    _manifest: &Manifest,
) -> Result<Vec<Item<'a>>> {
    unimplemented!()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Created,
    Updated,
    Overwrote,
    Kept(State),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    pub name: String,
    pub path: PathBuf,
    pub outcome: Outcome,
}

pub fn apply(
    _dest: &Path,
    _items: &[Item<'_>],
    _force: bool,
    _fetch: &mut dyn FnMut(&Entry) -> Result<Vec<u8>>,
    _manifest: &mut Manifest,
) -> Result<Vec<Done>> {
    unimplemented!()
}
