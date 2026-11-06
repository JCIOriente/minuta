use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::template::catalog::{sha256_hex, Catalog, Entry, Manifest, State};
use crate::template::pull::{
    apply, fetch_catalog, fetch_file, outcome_for, plan, read_manifest, Done, Origin, Outcome,
    CATALOG_CAP, FILE_CAP, LOCAL_CAP,
};
use tempfile::TempDir;

use crate::test_support::fake_http::{FakeHttp, Route};

mod apply_force;
mod apply_manifest;
mod apply_symlinks;
mod apply_verify;
mod apply_writes;
mod end_to_end;
mod fetch;
mod manifest;
mod origin;
mod outcome_for;
mod plan;

// ---- helpers ----

fn template_text(tag: &str) -> String {
    format!("---\ndescription: {tag}\n---\nWrite about {{{{job_name}}}} ({tag}).\n")
}

fn entry_for(name: &str, text: &str) -> Entry {
    Entry {
        name: name.to_string(),
        description: "d".to_string(),
        platform: None,
        tags: vec![],
        input: None,
        sha256: sha256_hex(text.as_bytes()),
    }
}

/// An entry for `name` whose catalog hash is that of `text`.
fn entry(name: &str, text: &str) -> Entry {
    entry_for(name, text)
}

/// Serves file contents by template name and records which were asked for.
struct Fetcher {
    files: BTreeMap<String, Vec<u8>>,
    calls: Vec<String>,
}

impl Fetcher {
    fn new(files: &[(&str, &[u8])]) -> Fetcher {
        Fetcher {
            files: files
                .iter()
                .map(|(n, b)| (n.to_string(), b.to_vec()))
                .collect(),
            calls: vec![],
        }
    }

    fn fetch(&mut self, e: &Entry) -> Result<Vec<u8>> {
        self.calls.push(e.name.clone());
        match self.files.get(&e.name) {
            Some(bytes) => Ok(bytes.clone()),
            None => Err(Error::Io(std::io::Error::other(format!(
                "no fake file for {}",
                e.name
            )))),
        }
    }
}

struct Dest {
    tmp: TempDir,
}

impl Dest {
    fn new() -> Dest {
        Dest {
            tmp: tempfile::tempdir().unwrap(),
        }
    }

    /// `<tmp>/templates`, not created.
    fn dir(&self) -> PathBuf {
        self.tmp.path().join("templates")
    }

    fn create(&self) -> PathBuf {
        fs::create_dir_all(self.dir()).unwrap();
        self.dir()
    }

    fn write(&self, file: &str, text: &str) -> PathBuf {
        let dir = self.create();
        let path = dir.join(file);
        fs::write(&path, text).unwrap();
        path
    }

    fn outside(&self, file: &str, text: &str) -> PathBuf {
        let path = self.tmp.path().join(file);
        fs::write(&path, text).unwrap();
        path
    }
}

fn run(
    dest: &Path,
    entries: &[&Entry],
    force: bool,
    fetcher: &mut Fetcher,
    manifest: &mut Manifest,
) -> Result<Vec<Done>> {
    let items = plan(dest, entries, manifest)?;
    apply(dest, &items, force, &mut |e| fetcher.fetch(e), manifest)
}

fn err_text<T: std::fmt::Debug>(r: Result<T>) -> String {
    match r {
        Err(e) => e.to_string(),
        Ok(v) => panic!("expected an error, got {v:?}"),
    }
}

fn manifest_with(pairs: &[(&str, &str)]) -> Manifest {
    let mut m = Manifest::default();
    for (name, text) in pairs {
        m.set(name, &sha256_hex(text.as_bytes()));
    }
    m
}

fn catalog_text(entries: &[&Entry]) -> String {
    let mut s = String::from("version = 1\n");
    for e in entries {
        s.push_str(&format!(
            "\n[[template]]\nname = \"{}\"\ndescription = \"d\"\nsha256 = \"{}\"\n",
            e.name, e.sha256
        ));
    }
    s
}
