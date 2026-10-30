//! The template catalog (`templates/index.toml` in the repo), the manifest of pulled files
//! (`.pulled.toml` next to them) and the state of each template. Pure functions: no I/O.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::valid_stem;
use crate::error::{Error, Result};

/// The only `version` of the catalog and of the manifest this minuta reads.
const VERSION: u32 = 1;

const MANIFEST_WARNING: &str = "invalid manifest, treating every file as modified: ";

/// One template offered by the catalog. `sha256` is the hash of the file's bytes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    pub description: String,
    pub platform: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// The template's `input:` chain. Informational: pull does not resolve it.
    pub input: Option<String>,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    pub entries: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFile {
    version: u32,
    #[serde(default)]
    template: Vec<Entry>,
}

impl Catalog {
    /// Parses `index.toml`. The text comes from the network: names and hashes are validated
    /// here, and any string put in an error is escaped with `{:?}`.
    pub fn parse(text: &str) -> Result<Catalog> {
        let fail = |message: String| Error::Parse(format!("template catalog: {message}"));
        let file: CatalogFile = toml::from_str(text).map_err(|e| fail(e.to_string()))?;
        if file.version != VERSION {
            return Err(fail(format!(
                "unsupported version {} (this minuta reads version {VERSION}; upgrade minuta)",
                file.version
            )));
        }
        let mut seen = HashSet::new();
        for entry in &file.template {
            if !valid_stem(&entry.name) {
                return Err(fail(format!(
                    "invalid template name {:?} (use lowercase letters, digits, '-' and '_', starting with a letter or digit)",
                    entry.name
                )));
            }
            if !seen.insert(entry.name.as_str()) {
                return Err(fail(format!("duplicate template {:?}", entry.name)));
            }
            if !is_sha256(&entry.sha256) {
                return Err(fail(format!(
                    "template {:?} has an invalid sha256 (expected 64 lowercase hex characters)",
                    entry.name
                )));
            }
        }
        Ok(Catalog {
            entries: file.template,
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestEntry {
    name: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestIn {
    version: u32,
    #[serde(default)]
    file: Vec<ManifestEntry>,
}

#[derive(Serialize)]
struct ManifestOut {
    version: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    file: Vec<ManifestEntry>,
}

/// The hash each pulled template had when it was written, by template name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    files: BTreeMap<String, String>,
}

impl Manifest {
    /// Reads `.pulled.toml`. Text that cannot be trusted gives an empty manifest and a
    /// warning, so every file on disk reads as modified and none is overwritten.
    pub fn parse(text: &str) -> (Manifest, Option<String>) {
        if text.trim().is_empty() {
            return (Manifest::default(), None);
        }
        match Manifest::try_parse(text) {
            Ok(manifest) => (manifest, None),
            Err(reason) => (
                Manifest::default(),
                Some(format!("{MANIFEST_WARNING}{reason}")),
            ),
        }
    }

    fn try_parse(text: &str) -> std::result::Result<Manifest, String> {
        let file: ManifestIn = toml::from_str(text).map_err(|e| e.to_string())?;
        if file.version != VERSION {
            return Err(format!("unsupported version {}", file.version));
        }
        let mut files = BTreeMap::new();
        for entry in file.file {
            if !valid_stem(&entry.name) {
                return Err(format!("invalid template name {:?}", entry.name));
            }
            if !is_sha256(&entry.sha256) {
                return Err(format!("template {:?} has an invalid sha256", entry.name));
            }
            if files.contains_key(&entry.name) {
                return Err(format!("duplicate template {:?}", entry.name));
            }
            files.insert(entry.name, entry.sha256);
        }
        Ok(Manifest { files })
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.files.get(name).map(String::as_str)
    }

    pub fn set(&mut self, name: &str, sha256: &str) {
        self.files.insert(name.to_string(), sha256.to_string());
    }

    /// The file's text, sorted by name so the output is stable.
    pub fn to_toml(&self) -> String {
        let out = ManifestOut {
            version: VERSION,
            file: self
                .files
                .iter()
                .map(|(name, sha256)| ManifestEntry {
                    name: name.clone(),
                    sha256: sha256.clone(),
                })
                .collect(),
        };
        toml::to_string(&out).expect("a version and a list of string pairs always serialize")
    }
}

/// Where a template in the destination folder stands against the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    New,
    UpToDate,
    UpdateAvailable,
    Modified,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            State::New => "new",
            State::UpToDate => "up to date",
            State::UpdateAvailable => "update available",
            State::Modified => "modified",
        }
    }
}

/// `local` is the hash of the file on disk (None if absent), `manifest` the hash recorded
/// when it was pulled, `catalog` the hash the catalog offers now.
pub fn state(local: Option<&str>, manifest: Option<&str>, catalog: &str) -> State {
    match local {
        None => State::New,
        Some(local) if local == catalog => State::UpToDate,
        Some(local) if manifest == Some(local) => State::UpdateAvailable,
        Some(_) => State::Modified,
    }
}

pub enum Selection<'a> {
    Names(&'a [String]),
    All,
    /// Case-insensitive text matched against name, platform and tags.
    Filter(&'a str),
}

pub fn select<'c>(catalog: &'c Catalog, selection: Selection<'_>) -> Result<Vec<&'c Entry>> {
    match selection {
        Selection::All => Ok(catalog.entries.iter().collect()),
        Selection::Names(names) => {
            let mut chosen: Vec<&Entry> = Vec::new();
            for name in names {
                let entry = catalog
                    .entries
                    .iter()
                    .find(|e| e.name == *name)
                    .ok_or_else(|| {
                        let available: Vec<&str> =
                            catalog.entries.iter().map(|e| e.name.as_str()).collect();
                        Error::Usage(format!(
                            "unknown template {name:?} (available: {})",
                            available.join(", ")
                        ))
                    })?;
                if !chosen.iter().any(|c| c.name == entry.name) {
                    chosen.push(entry);
                }
            }
            Ok(chosen)
        }
        Selection::Filter(text) => {
            let needle = text.to_lowercase();
            let hit = |s: &str| s.to_lowercase().contains(&needle);
            let chosen: Vec<&Entry> = catalog
                .entries
                .iter()
                .filter(|e| {
                    hit(&e.name)
                        || e.platform.as_deref().is_some_and(hit)
                        || e.tags.iter().any(|t| hit(t))
                })
                .collect();
            if chosen.is_empty() {
                return Err(Error::Usage(format!("no template matches filter {text:?}")));
            }
            Ok(chosen)
        }
    }
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
