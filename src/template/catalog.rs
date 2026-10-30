//! The template catalog (`templates/index.toml` in the repo), the manifest of pulled files
//! (`.pulled.toml` next to them) and the state of each template. Pure functions: no I/O.

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub description: String,
    pub platform: Option<String>,
    pub tags: Vec<String>,
    pub input: Option<String>,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    pub entries: Vec<Entry>,
}

impl Catalog {
    pub fn parse(_text: &str) -> Result<Catalog> {
        unimplemented!()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    files: std::collections::BTreeMap<String, String>,
}

impl Manifest {
    pub fn parse(_text: &str) -> (Manifest, Option<String>) {
        unimplemented!()
    }

    pub fn get(&self, _name: &str) -> Option<&str> {
        unimplemented!()
    }

    pub fn set(&mut self, _name: &str, _sha256: &str) {
        unimplemented!()
    }

    pub fn to_toml(&self) -> String {
        unimplemented!()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    New,
    UpToDate,
    UpdateAvailable,
    Modified,
}

impl State {
    pub fn label(self) -> &'static str {
        unimplemented!()
    }
}

pub fn state(_local: Option<&str>, _manifest: Option<&str>, _catalog: &str) -> State {
    unimplemented!()
}

pub enum Selection<'a> {
    Names(&'a [String]),
    All,
    Filter(&'a str),
}

pub fn select<'c>(_catalog: &'c Catalog, _selection: Selection<'_>) -> Result<Vec<&'c Entry>> {
    unimplemented!()
}

pub fn sha256_hex(_bytes: &[u8]) -> String {
    unimplemented!()
}
