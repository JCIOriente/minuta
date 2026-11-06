use super::*;
use crate::template::catalog::{sha256_hex, State};
use std::fs;
use std::io::Cursor;

use crate::error::{Error, Result};
use crate::progress::Mode;
use crate::template::catalog::Manifest;
use crate::template::pull::Origin;
use crate::template::pull_cmd::{parse_choice, run, Choice, Options, Terminal};
use crate::test_support::fake_http::{FakeHttp, Route};
use tempfile::TempDir;

mod choosing;
mod list;
mod missing_input;
mod numbered_list;
mod parse_choice;
mod progress;
mod prompt;

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

// ---- helpers for the tests in the files above ----

fn template_text(name: &str) -> String {
    format!("---\ndescription: {name}\n---\nWrite about {{{{job_name}}}} ({name}).\n")
}

/// `(name, platform, description, input)` of the three catalog entries.
const ENTRIES: [(&str, Option<&str>, &str, Option<&str>); 3] = [
    ("alpha", Some("social"), "Alpha post", None),
    ("beta", None, "Beta notes", None),
    ("gamma", Some("audio"), "Gamma audio", None),
];

fn catalog_text(entries: &[(&str, Option<&str>, &str, Option<&str>)]) -> String {
    let mut s = String::from("version = 1\n");
    for (name, platform, description, input) in entries {
        s.push_str(&format!(
            "\n[[template]]\nname = \"{name}\"\ndescription = \"{description}\"\nsha256 = \"{}\"\n",
            sha256_hex(template_text(name).as_bytes())
        ));
        if let Some(platform) = platform {
            s.push_str(&format!("platform = \"{platform}\"\n"));
        }
        if let Some(input) = input {
            s.push_str(&format!("input = \"{input}\"\n"));
        }
    }
    s
}

fn server_with(entries: &[(&str, Option<&str>, &str, Option<&str>)]) -> FakeHttp {
    let mut routes = vec![Route::ok("/templates/index.toml", catalog_text(entries))];
    for (name, ..) in entries {
        routes.push(Route::ok(
            &format!("/templates/{name}.md"),
            template_text(name),
        ));
    }
    FakeHttp::start(routes)
}

fn server() -> FakeHttp {
    server_with(&ENTRIES)
}

fn sha(name: &str) -> String {
    sha256_hex(template_text(name).as_bytes())
}

struct Case {
    choice: Choice,
    list: bool,
    force: bool,
    yes: bool,
    interactive: bool,
    input: &'static str,
    known: Option<Vec<String>>,
    mode: Mode,
}

fn case(choice: Choice) -> Case {
    Case {
        choice,
        list: false,
        force: false,
        yes: false,
        interactive: false,
        input: "",
        known: Some(vec![]),
        mode: Mode::Plain,
    }
}

fn names(list: &[&str]) -> Choice {
    Choice::Names(list.iter().map(|s| s.to_string()).collect())
}

struct Out {
    stdout: String,
    stderr: String,
    result: Result<()>,
}

impl Case {
    fn run(&self, server: &FakeHttp, dest: &Path) -> Out {
        let origin = Origin::new(&server.base()).unwrap();
        let options = Options {
            choice: self.choice.clone(),
            list: self.list,
            dest: dest.to_path_buf(),
            force: self.force,
            yes: self.yes,
            known: self.known.as_deref(),
            mode: self.mode,
        };
        let mut input = Cursor::new(self.input.as_bytes().to_vec());
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let result = run(
            &origin,
            &options,
            &mut Terminal {
                interactive: self.interactive,
                input: &mut input,
                stdout: &mut stdout,
                stderr: &mut stderr,
            },
        );
        Out {
            stdout: String::from_utf8(stdout).unwrap(),
            stderr: String::from_utf8(stderr).unwrap(),
            result,
        }
    }
}

fn usage(r: &Result<()>) -> String {
    match r {
        Err(Error::Usage(m)) => m.clone(),
        other => panic!("expected usage error, got {other:?}"),
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

    fn dir(&self) -> PathBuf {
        self.tmp.path().join("templates")
    }

    fn write(&self, file: &str, text: &str) {
        fs::create_dir_all(self.dir()).unwrap();
        fs::write(self.dir().join(file), text).unwrap();
    }

    fn read(&self, file: &str) -> String {
        fs::read_to_string(self.dir().join(file)).unwrap()
    }

    /// A file minuta pulled earlier: on disk and in the manifest.
    fn pulled(&self, name: &str, text: &str) {
        self.write(&format!("{name}.md"), text);
        let path = self.dir().join(".pulled.toml");
        let mut manifest = if path.exists() {
            Manifest::parse(&fs::read_to_string(&path).unwrap()).0
        } else {
            Manifest::default()
        };
        manifest.set(name, &sha256_hex(text.as_bytes()));
        fs::write(path, manifest.to_toml()).unwrap();
    }
}

const LIST: &str =
    " 1) alpha  new  Alpha post\n 2) beta   new  Beta notes\n 3) gamma  new  Gamma audio\n";
const ASK: &str =
    "Select templates (numbers separated by commas or spaces, a for all, empty to cancel): ";
