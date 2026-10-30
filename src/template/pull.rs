//! Downloading curated templates: the origin they come from, the plan of what to write, and
//! the verified, all-or-nothing write. No prompts and no printing: the CLI owns those.
//!
//! The flow is `fetch_catalog`, select (see `catalog::select`), `read_manifest`, `plan`, then
//! `apply`, which fetches only what it will write.

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::catalog::{self, sha256_hex, Catalog, Entry, Manifest, State};
use super::{valid_stem, Layer, Template};
use crate::error::{Error, Result};
use crate::job::write_atomic;

/// Largest `index.toml` accepted.
pub const CATALOG_CAP: u64 = 1024 * 1024;
/// Largest template file accepted.
pub const FILE_CAP: u64 = 256 * 1024;

const GITHUB: &str = "https://raw.githubusercontent.com/JCIOriente/minuta";
const MANIFEST_FILE: &str = ".pulled.toml";

/// How long to wait for a connection and for each read, like `models.rs`.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Where the catalog and the template files are downloaded from: a folder URL that holds
/// `index.toml` and `<name>.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    base: String,
}

impl Origin {
    /// `base` must be an https URL, or http on 127.0.0.1 (used by tests). A trailing `/` is
    /// ignored.
    pub fn new(base: &str) -> Result<Origin> {
        let trimmed = base.trim_end_matches('/');
        if base.chars().any(|c| c.is_whitespace() || c.is_control()) || !allowed_base(trimmed) {
            return Err(Error::Usage(format!(
                "templates URL {base:?} must use https (http is only allowed for 127.0.0.1)"
            )));
        }
        Ok(Origin {
            base: trimmed.to_string(),
        })
    }

    /// The repository's `templates/` folder at `reference` (a branch, tag or commit).
    pub fn github(reference: &str) -> Result<Origin> {
        if !valid_ref(reference) {
            return Err(Error::Usage(format!(
                "invalid ref {reference:?} (use letters, digits, '.', '_', '-' and '/')"
            )));
        }
        Origin::new(&format!("{GITHUB}/{reference}/templates"))
    }

    /// The host (and port, if any) files come from, for the confirmation prompt.
    pub fn host(&self) -> &str {
        let rest = self
            .base
            .split_once("://")
            .map_or(self.base.as_str(), |(_, rest)| rest);
        rest.split('/').next().unwrap_or(rest)
    }

    pub fn url(&self, file: &str) -> String {
        format!("{}/{file}", self.base)
    }
}

/// An https URL with a plain host, or http to 127.0.0.1 (optionally with a port).
fn allowed_base(base: &str) -> bool {
    if let Some(rest) = base.strip_prefix("https://") {
        let authority = rest.split('/').next().unwrap_or_default();
        return !authority.is_empty() && !authority.contains('@');
    }
    let Some(rest) = base.strip_prefix("http://127.0.0.1") else {
        return false;
    };
    match rest.chars().next() {
        None | Some('/') => true,
        Some(':') => {
            let port = rest[1..].split('/').next().unwrap_or_default();
            !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit())
        }
        Some(_) => false,
    }
}

/// A branch, tag or commit name that cannot change the meaning of the URL path.
fn valid_ref(reference: &str) -> bool {
    !reference.is_empty()
        && reference
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && reference
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
}

fn io_other(message: String) -> Error {
    Error::Io(io::Error::other(message))
}

fn symlink_error(path: &Path) -> Error {
    io_other(format!(
        "{} is a symlink; refusing to use it",
        path.display()
    ))
}

/// GETs `url`, accepting only a 200 (redirects are not followed) of at most `cap` bytes.
fn get(url: &str, cap: u64) -> Result<Vec<u8>> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .redirects(0)
        .build();
    let failed = |reason: String| io_other(format!("download of {url} failed: {reason}"));
    let response = agent.get(url).call().map_err(|e| match &e {
        ureq::Error::Status(code, _) => failed(format!("HTTP status {code}")),
        ureq::Error::Transport(_) => failed(e.to_string()),
    })?;
    if response.status() != 200 {
        return Err(failed(format!("HTTP status {}", response.status())));
    }
    let mut body = Vec::new();
    response
        .into_reader()
        .take(cap + 1)
        .read_to_end(&mut body)?;
    if body.len() as u64 > cap {
        return Err(io_other(format!(
            "response from {url} is larger than {cap} bytes"
        )));
    }
    Ok(body)
}

pub fn fetch_catalog(origin: &Origin) -> Result<Catalog> {
    let bytes = get(&origin.url("index.toml"), CATALOG_CAP)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| Error::Parse("template catalog: the file is not valid UTF-8".into()))?;
    Catalog::parse(&text)
}

pub fn fetch_file(origin: &Origin, entry: &Entry) -> Result<Vec<u8>> {
    check_name(&entry.name)?;
    get(&origin.url(&format!("{}.md", entry.name)), FILE_CAP)
}

/// `Entry` has public fields, so a name that `Catalog::parse` would have refused can still
/// arrive here. It must never become a path or a URL.
fn check_name(name: &str) -> Result<()> {
    if valid_stem(name) {
        return Ok(());
    }
    Err(Error::Usage(format!(
        "invalid template name {name:?} (use lowercase letters, digits, '-' and '_', starting with a letter or digit)"
    )))
}

/// Reads `<dest>/.pulled.toml`. A missing file (or folder) is an empty manifest. A file that
/// cannot be trusted is an empty manifest plus a warning (see `Manifest::parse`). A file that
/// cannot be read, or a symlink, is an error.
pub fn read_manifest(dest: &Path) -> Result<(Manifest, Option<String>)> {
    let path = dest.join(MANIFEST_FILE);
    let cannot_read = |e: io::Error| io_other(format!("cannot read {}: {e}", path.display()));
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => return Err(symlink_error(&path)),
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((Manifest::default(), None)),
        Err(e) => return Err(cannot_read(e)),
    }
    let bytes = fs::read(&path).map_err(cannot_read)?;
    // Bytes that are not UTF-8 are not TOML either; the replacement characters make
    // `Manifest::parse` reject the text and give the usual warning.
    Ok(Manifest::parse(&String::from_utf8_lossy(&bytes)))
}

/// One selected template and what is already in the destination folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item<'a> {
    pub entry: &'a Entry,
    pub path: PathBuf,
    pub state: State,
}

/// Looks at the destination folder and says where each selected template stands. Writes
/// nothing. The folder may be missing (everything is then new); a symlink or a file where
/// the folder should be is an error, and so is a symlink or a folder where a template
/// should be.
pub fn plan<'a>(
    dest: &Path,
    selection: &[&'a Entry],
    manifest: &Manifest,
) -> Result<Vec<Item<'a>>> {
    match fs::symlink_metadata(dest) {
        Ok(meta) if meta.file_type().is_symlink() => return Err(symlink_error(dest)),
        Ok(meta) if !meta.is_dir() => {
            return Err(io_other(format!(
                "{} exists and is not a directory",
                dest.display()
            )))
        }
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_other(format!("cannot read {}: {e}", dest.display()))),
    }
    let mut items = Vec::with_capacity(selection.len());
    for &entry in selection {
        check_name(&entry.name)?;
        let path = dest.join(format!("{}.md", entry.name));
        let local = local_hash(&path)?;
        let state = catalog::state(local.as_deref(), manifest.get(&entry.name), &entry.sha256);
        items.push(Item { entry, path, state });
    }
    Ok(items)
}

/// The hash of the file at `path`, or None when there is none.
fn local_hash(path: &Path) -> Result<Option<String>> {
    let cannot_read = |e: io::Error| io_other(format!("cannot read {}: {e}", path.display()));
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(symlink_error(path)),
        Ok(meta) if !meta.is_file() => Err(io_other(format!(
            "{} is not a regular file",
            path.display()
        ))),
        Ok(_) => Ok(Some(sha256_hex(&fs::read(path).map_err(cannot_read)?))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(cannot_read(e)),
    }
}

/// What happened to one template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Written for the first time.
    Created,
    /// Replaced by a newer version of a file minuta had written and nobody had edited.
    Updated,
    /// A modified file replaced because of `force`; the old one is in `NAME.md.bak`.
    Overwrote,
    /// Left alone, in this state (up to date, or modified without `force`).
    Kept(State),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    pub name: String,
    pub path: PathBuf,
    pub outcome: Outcome,
}

/// Writes what the plan says to write: new and updatable files, and modified ones when
/// `force` is set. Order: fetch every file and verify it (catalog hash, UTF-8, parses as a
/// template), save backups of the modified files that will be replaced, then write. A
/// failure before the writes leaves the folder untouched. Each write is a temp file plus a
/// rename, so no half file appears. `manifest` gets an entry for every file written, and
/// `.pulled.toml` is rewritten after the files, only when something was written.
pub fn apply(
    dest: &Path,
    items: &[Item<'_>],
    force: bool,
    fetch: &mut dyn FnMut(&Entry) -> Result<Vec<u8>>,
    manifest: &mut Manifest,
) -> Result<Vec<Done>> {
    let outcome = |item: &Item<'_>| match (item.state, force) {
        (State::New, _) => Outcome::Created,
        (State::UpdateAvailable, _) => Outcome::Updated,
        (State::Modified, true) => Outcome::Overwrote,
        (state, _) => Outcome::Kept(state),
    };
    let writes = |item: &Item<'_>| !matches!(outcome(item), Outcome::Kept(_));

    let mut fetched: Vec<(&Item<'_>, Vec<u8>)> = Vec::new();
    for item in items.iter().filter(|i| writes(i)) {
        let bytes = fetch(item.entry)?;
        verify(item.entry, &bytes)?;
        fetched.push((item, bytes));
    }

    for (item, _) in fetched.iter().filter(|(i, _)| i.state == State::Modified) {
        let bak = backup_path(&item.path);
        let old = fs::read(&item.path)
            .map_err(|e| io_other(format!("cannot read {}: {e}", item.path.display())))?;
        write_file(&bak, &old)
            .map_err(|e| io_other(format!("cannot save a backup to {}: {e}", bak.display())))?;
    }

    fs::create_dir_all(dest)
        .map_err(|e| io_other(format!("cannot create {}: {e}", dest.display())))?;
    let mut written = 0usize;
    let mut failure = None;
    for (item, bytes) in &fetched {
        match write_file(&item.path, bytes) {
            Ok(()) => {
                manifest.set(&item.entry.name, &item.entry.sha256);
                written += 1;
            }
            Err(e) => {
                failure = Some(io_other(format!(
                    "cannot write {}: {e}",
                    item.path.display()
                )));
                break;
            }
        }
    }
    // Record what did land, even when a later write failed, so the manifest stays true.
    if written > 0 {
        let path = dest.join(MANIFEST_FILE);
        if let Err(e) = write_file(&path, manifest.to_toml().as_bytes()) {
            failure
                .get_or_insert_with(|| io_other(format!("cannot write {}: {e}", path.display())));
        }
    }
    if let Some(e) = failure {
        return Err(e);
    }

    Ok(items
        .iter()
        .map(|item| Done {
            name: item.entry.name.clone(),
            path: item.path.clone(),
            outcome: outcome(item),
        })
        .collect())
}

/// Checks a downloaded file before anything is written.
fn verify(entry: &Entry, bytes: &[u8]) -> Result<()> {
    let fail = |message: String| Error::Template {
        name: entry.name.clone(),
        message,
    };
    let actual = sha256_hex(bytes);
    if actual != entry.sha256 {
        return Err(fail(format!(
            "downloaded file does not match the catalog checksum (expected {}, got {actual})",
            entry.sha256
        )));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| fail("downloaded file is not valid UTF-8".into()))?;
    let file = PathBuf::from(format!("{}.md", entry.name));
    Template::parse(&entry.name, Layer::Project, Some(file), text).map_err(|e| {
        let reason = match e {
            Error::Usage(m) => m
                .strip_prefix(&format!("template {}: ", entry.name))
                .map_or_else(|| m.clone(), str::to_string),
            other => other.to_string(),
        };
        fail(format!(
            "invalid template from the catalog: {reason}; if it uses a newer format, upgrade minuta"
        ))
    })?;
    Ok(())
}

fn backup_path(path: &Path) -> PathBuf {
    let mut bak = path.as_os_str().to_owned();
    bak.push(".bak");
    PathBuf::from(bak)
}

/// Replaces `path` with `bytes` through a temp file next to it. A stale temp file (or a
/// symlink planted there by an untrusted folder) is removed first and the new one is
/// created exclusively, so the write can never go through a link.
fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic(path, |tmp| {
        match fs::remove_file(tmp) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let mut file = OpenOptions::new().write(true).create_new(true).open(tmp)?;
        file.write_all(bytes)?;
        Ok(())
    })
    .map_err(|e| match e {
        Error::Io(e) => e,
        other => io::Error::other(other.to_string()),
    })
}
