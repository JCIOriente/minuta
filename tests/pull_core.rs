use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use minuta::error::{Error, Result};
use minuta::template::catalog::{sha256_hex, Catalog, Entry, Manifest, State};
use minuta::template::pull::{
    apply, fetch_catalog, fetch_file, plan, read_manifest, Done, Origin, Outcome, CATALOG_CAP,
    FILE_CAP,
};
use tempfile::TempDir;

#[path = "support/fake_http.rs"]
mod fake_http;

use fake_http::{FakeHttp, Route};

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

// ---- origin ----

#[test]
fn origin_github_url() {
    let origin = Origin::github("main").unwrap();
    assert_eq!(
        origin.url("index.toml"),
        "https://raw.githubusercontent.com/JCIOriente/minuta/main/templates/index.toml"
    );
}

#[test]
fn origin_github_host() {
    assert_eq!(
        Origin::github("main").unwrap().host(),
        "raw.githubusercontent.com"
    );
}

#[test]
fn origin_accepts_ref_forms() {
    for reference in [
        "main",
        "v0.3.0",
        "feature/x-1",
        "release_2",
        "0123456789abcdef0123456789abcdef01234567",
    ] {
        let origin = Origin::github(reference).unwrap();
        assert_eq!(
            origin.url("a.md"),
            format!(
                "https://raw.githubusercontent.com/JCIOriente/minuta/{reference}/templates/a.md"
            ),
            "ref {reference:?}"
        );
    }
}

#[test]
fn origin_rejects_bad_refs() {
    for reference in [
        "", "a b", "../x", "x/../y", "/abs", "a?b", "a#b", "a%2fb", "a\nb", "x/", "a//b", ".", "..",
    ] {
        match Origin::github(reference) {
            Err(Error::Usage(m)) => assert_eq!(
                m,
                format!("invalid ref {reference:?} (use letters, digits, '.', '_', '-' and '/')"),
                "ref {reference:?}"
            ),
            other => panic!("ref {reference:?}: expected usage error, got {other:?}"),
        }
    }
}

#[test]
fn origin_new_rejects_plain_http_elsewhere_and_other_schemes() {
    for base in [
        "http://example.com/t",
        "http://localhost:1/t",
        "http://127.0.0.1.evil.com/t",
        "http://127.0.0.10/t",
        "ftp://example.com/t",
        "file:///tmp/t",
        "example.com/t",
        "",
        "https://",
        "https://user@host.example/t",
        "https://host.example/a b",
        "https://host.example/a\nb",
    ] {
        match Origin::new(base) {
            Err(Error::Usage(m)) => assert_eq!(
                m,
                format!(
                    "templates URL {base:?} must use https (http is only allowed for 127.0.0.1)"
                ),
                "base {base:?}"
            ),
            other => panic!("base {base:?}: expected usage error, got {other:?}"),
        }
    }
}

#[test]
fn origin_new_accepts_https_and_loopback_http() {
    assert_eq!(
        Origin::new("https://example.com/t").unwrap().host(),
        "example.com"
    );
    assert_eq!(
        Origin::new("http://127.0.0.1:8080/templates")
            .unwrap()
            .host(),
        "127.0.0.1:8080"
    );
    assert_eq!(
        Origin::new("http://127.0.0.1/templates").unwrap().host(),
        "127.0.0.1"
    );
}

#[test]
fn origin_trailing_slash_is_trimmed() {
    let origin = Origin::new("https://h.example/t/").unwrap();
    assert_eq!(origin.url("a.md"), "https://h.example/t/a.md");
}

// ---- fetch ----

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

#[test]
fn fetch_catalog_parses() {
    let a = entry("alpha", &template_text("a"));
    let b = entry("beta", &template_text("b"));
    let server = FakeHttp::start(vec![Route::ok(
        "/templates/index.toml",
        catalog_text(&[&a, &b]),
    )]);
    let catalog = fetch_catalog(&Origin::new(&server.base()).unwrap()).unwrap();
    assert_eq!(catalog.entries, vec![a, b]);
    assert_eq!(server.requests(), vec!["/templates/index.toml"]);
}

#[test]
fn fetch_catalog_http_error_names_the_url() {
    let server = FakeHttp::start(vec![]);
    let base = server.base();
    let text = err_text(fetch_catalog(&Origin::new(&base).unwrap()));
    assert_eq!(
        text,
        format!("I/O error: download of {base}/index.toml failed: HTTP status 404")
    );
}

#[test]
fn fetch_catalog_invalid_toml_is_a_parse_error() {
    let server = FakeHttp::start(vec![Route::ok("/templates/index.toml", "version = [")]);
    match fetch_catalog(&Origin::new(&server.base()).unwrap()) {
        Err(Error::Parse(m)) => assert!(m.starts_with("template catalog: "), "{m}"),
        other => panic!("expected parse error, got {other:?}"),
    }
}

#[test]
fn fetch_catalog_not_utf8_is_a_parse_error() {
    let server = FakeHttp::start(vec![Route::ok(
        "/templates/index.toml",
        vec![0xff, 0xfe, 0xfd],
    )]);
    match fetch_catalog(&Origin::new(&server.base()).unwrap()) {
        Err(Error::Parse(m)) => assert_eq!(m, "template catalog: the file is not valid UTF-8"),
        other => panic!("expected parse error, got {other:?}"),
    }
}

fn padded_catalog(len: u64) -> String {
    let mut s = String::from("version = 1\n");
    while (s.len() as u64) + 2 <= len {
        s.push_str("#\n");
    }
    while (s.len() as u64) < len {
        s.push('#');
    }
    assert_eq!(s.len() as u64, len);
    s
}

#[test]
fn fetch_catalog_accepts_a_body_of_exactly_the_cap() {
    let server = FakeHttp::start(vec![Route::ok(
        "/templates/index.toml",
        padded_catalog(CATALOG_CAP),
    )]);
    let catalog = fetch_catalog(&Origin::new(&server.base()).unwrap()).unwrap();
    assert_eq!(catalog.entries, vec![]);
}

#[test]
fn fetch_catalog_rejects_a_body_over_the_cap() {
    let server = FakeHttp::start(vec![Route::ok(
        "/templates/index.toml",
        padded_catalog(CATALOG_CAP + 1),
    )]);
    let base = server.base();
    assert_eq!(
        err_text(fetch_catalog(&Origin::new(&base).unwrap())),
        format!("I/O error: response from {base}/index.toml is larger than 1048576 bytes")
    );
}

#[test]
fn fetch_file_returns_the_bytes() {
    let text = template_text("a");
    let a = entry("alpha", &text);
    let server = FakeHttp::start(vec![Route::ok("/templates/alpha.md", text.clone())]);
    let bytes = fetch_file(&Origin::new(&server.base()).unwrap(), &a).unwrap();
    assert_eq!(bytes, text.as_bytes());
    assert_eq!(server.requests(), vec!["/templates/alpha.md"]);
}

#[test]
fn fetch_file_accepts_a_body_of_exactly_the_cap() {
    let a = entry("alpha", "x");
    let body = vec![b'a'; FILE_CAP as usize];
    let server = FakeHttp::start(vec![Route::ok("/templates/alpha.md", body.clone())]);
    assert_eq!(
        fetch_file(&Origin::new(&server.base()).unwrap(), &a).unwrap(),
        body
    );
}

#[test]
fn fetch_file_rejects_a_body_over_the_cap() {
    let a = entry("alpha", "x");
    let server = FakeHttp::start(vec![Route::ok(
        "/templates/alpha.md",
        vec![b'a'; FILE_CAP as usize + 1],
    )]);
    let base = server.base();
    assert_eq!(
        err_text(fetch_file(&Origin::new(&base).unwrap(), &a)),
        format!("I/O error: response from {base}/alpha.md is larger than 262144 bytes")
    );
}

#[test]
fn fetch_file_rejects_a_name_the_catalog_would_not_have_allowed() {
    let mut a = entry("alpha", "x");
    a.name = "../x".to_string();
    let origin = Origin::new("https://h.example/t").unwrap();
    match fetch_file(&origin, &a) {
        Err(Error::Usage(m)) => assert_eq!(
            m,
            "invalid template name \"../x\" (use lowercase letters, digits, '-' and '_', starting with a letter or digit)"
        ),
        other => panic!("expected usage error, got {other:?}"),
    }
}

#[test]
fn fetch_file_http_error_names_the_url() {
    let a = entry("alpha", "x");
    let server = FakeHttp::start(vec![Route::status("/templates/alpha.md", 500, "boom")]);
    let base = server.base();
    assert_eq!(
        err_text(fetch_file(&Origin::new(&base).unwrap(), &a)),
        format!("I/O error: download of {base}/alpha.md failed: HTTP status 500")
    );
}

#[test]
fn fetch_does_not_follow_redirects() {
    let a = entry("alpha", "x");
    let server = FakeHttp::start(vec![
        Route::redirect("/templates/alpha.md", "/templates/other.md"),
        Route::ok("/templates/other.md", "elsewhere"),
    ]);
    let base = server.base();
    assert_eq!(
        err_text(fetch_file(&Origin::new(&base).unwrap(), &a)),
        format!("I/O error: download of {base}/alpha.md failed: HTTP status 302")
    );
    assert_eq!(server.requests(), vec!["/templates/alpha.md"]);
}

// ---- manifest on disk ----

#[test]
fn read_manifest_without_a_file_is_empty() {
    let d = Dest::new();
    d.create();
    let (manifest, warning) = read_manifest(&d.dir()).unwrap();
    assert_eq!(manifest, Manifest::default());
    assert_eq!(warning, None);
}

#[test]
fn read_manifest_without_the_folder_is_empty() {
    let d = Dest::new();
    let (manifest, warning) = read_manifest(&d.dir()).unwrap();
    assert_eq!(manifest, Manifest::default());
    assert_eq!(warning, None);
}

#[test]
fn read_manifest_reads_a_valid_file() {
    let d = Dest::new();
    let m = manifest_with(&[("alpha", "a")]);
    d.write(".pulled.toml", &m.to_toml());
    let (read, warning) = read_manifest(&d.dir()).unwrap();
    assert_eq!(read, m);
    assert_eq!(warning, None);
}

#[test]
fn read_manifest_invalid_file_is_empty_with_a_warning() {
    let d = Dest::new();
    d.write(".pulled.toml", "version = [");
    let (manifest, warning) = read_manifest(&d.dir()).unwrap();
    assert_eq!(manifest, Manifest::default());
    let w = warning.expect("a warning");
    assert!(
        w.starts_with("invalid manifest, treating every file as modified: "),
        "{w}"
    );
}

#[test]
fn read_manifest_not_utf8_is_empty_with_a_warning() {
    let d = Dest::new();
    let dir = d.create();
    fs::write(dir.join(".pulled.toml"), [0xff, 0xfe]).unwrap();
    let (manifest, warning) = read_manifest(&dir).unwrap();
    assert_eq!(manifest, Manifest::default());
    let w = warning.expect("a warning");
    assert!(
        w.starts_with("invalid manifest, treating every file as modified: "),
        "{w}"
    );
}

#[test]
fn read_manifest_io_error_names_the_path() {
    let d = Dest::new();
    let dir = d.create();
    let path = dir.join(".pulled.toml");
    fs::create_dir(&path).unwrap(); // reading a directory fails with something other than NotFound
    let text = err_text(read_manifest(&dir));
    assert!(
        text.starts_with(&format!("I/O error: cannot read {}: ", path.display())),
        "{text}"
    );
}

#[cfg(unix)]
#[test]
fn read_manifest_refuses_a_symlink() {
    let d = Dest::new();
    let dir = d.create();
    let secret = d.outside("secret.txt", "version = 1\n");
    let path = dir.join(".pulled.toml");
    std::os::unix::fs::symlink(&secret, &path).unwrap();
    assert_eq!(
        err_text(read_manifest(&dir)),
        format!(
            "I/O error: {} is a symlink; refusing to use it",
            path.display()
        )
    );
}

// ---- plan ----

#[test]
fn plan_new_when_the_file_is_missing() {
    let d = Dest::new();
    d.create();
    let a = entry("alpha", &template_text("a"));
    let items = plan(&d.dir(), &[&a], &Manifest::default()).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].entry, &a);
    assert_eq!(items[0].path, d.dir().join("alpha.md"));
    assert_eq!(items[0].state, State::New);
}

#[test]
fn plan_new_when_the_folder_is_missing() {
    let d = Dest::new();
    let a = entry("alpha", &template_text("a"));
    let items = plan(&d.dir(), &[&a], &Manifest::default()).unwrap();
    assert_eq!(items[0].state, State::New);
}

#[test]
fn plan_up_to_date() {
    let d = Dest::new();
    let text = template_text("a");
    d.write("alpha.md", &text);
    let a = entry("alpha", &text);
    let items = plan(&d.dir(), &[&a], &manifest_with(&[("alpha", &text)])).unwrap();
    assert_eq!(items[0].state, State::UpToDate);
}

#[test]
fn plan_update_available() {
    let d = Dest::new();
    let old = template_text("old");
    d.write("alpha.md", &old);
    let a = entry("alpha", &template_text("new"));
    let items = plan(&d.dir(), &[&a], &manifest_with(&[("alpha", &old)])).unwrap();
    assert_eq!(items[0].state, State::UpdateAvailable);
}

#[test]
fn plan_modified_when_the_file_was_edited() {
    let d = Dest::new();
    let pulled = template_text("pulled");
    d.write("alpha.md", &template_text("edited by the user"));
    let a = entry("alpha", &template_text("new"));
    let items = plan(&d.dir(), &[&a], &manifest_with(&[("alpha", &pulled)])).unwrap();
    assert_eq!(items[0].state, State::Modified);
}

#[test]
fn plan_modified_when_minuta_never_wrote_the_file() {
    let d = Dest::new();
    d.write("alpha.md", &template_text("mine"));
    let a = entry("alpha", &template_text("new"));
    let items = plan(&d.dir(), &[&a], &Manifest::default()).unwrap();
    assert_eq!(items[0].state, State::Modified);
}

#[test]
fn plan_keeps_selection_order() {
    let d = Dest::new();
    let a = entry("alpha", &template_text("a"));
    let b = entry("beta", &template_text("b"));
    let items = plan(&d.dir(), &[&b, &a], &Manifest::default()).unwrap();
    let names: Vec<&str> = items.iter().map(|i| i.entry.name.as_str()).collect();
    assert_eq!(names, ["beta", "alpha"]);
}

#[test]
fn plan_destination_that_is_a_file_is_an_error() {
    let d = Dest::new();
    fs::write(d.dir(), "not a folder").unwrap();
    let a = entry("alpha", &template_text("a"));
    assert_eq!(
        err_text(plan(&d.dir(), &[&a], &Manifest::default())),
        format!(
            "I/O error: {} exists and is not a directory",
            d.dir().display()
        )
    );
}

#[cfg(unix)]
#[test]
fn plan_destination_that_is_a_symlink_is_refused() {
    let d = Dest::new();
    let real = d.tmp.path().join("real");
    fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, d.dir()).unwrap();
    let a = entry("alpha", &template_text("a"));
    assert_eq!(
        err_text(plan(&d.dir(), &[&a], &Manifest::default())),
        format!(
            "I/O error: {} is a symlink; refusing to use it",
            d.dir().display()
        )
    );
}

#[cfg(unix)]
#[test]
fn plan_template_that_is_a_symlink_is_refused() {
    let d = Dest::new();
    let dir = d.create();
    let target = d.outside("elsewhere.md", "x");
    let path = dir.join("alpha.md");
    std::os::unix::fs::symlink(&target, &path).unwrap();
    let a = entry("alpha", &template_text("a"));
    assert_eq!(
        err_text(plan(&dir, &[&a], &Manifest::default())),
        format!(
            "I/O error: {} is a symlink; refusing to use it",
            path.display()
        )
    );
}

#[test]
fn plan_template_that_is_a_directory_is_an_error() {
    let d = Dest::new();
    let dir = d.create();
    let path = dir.join("alpha.md");
    fs::create_dir(&path).unwrap();
    let a = entry("alpha", &template_text("a"));
    assert_eq!(
        err_text(plan(&dir, &[&a], &Manifest::default())),
        format!("I/O error: {} is not a regular file", path.display())
    );
}

// ---- apply: writing ----

#[test]
fn apply_writes_a_new_file_and_records_it() {
    let d = Dest::new();
    let text = template_text("a");
    let a = entry("alpha", &text);
    let mut f = Fetcher::new(&[("alpha", text.as_bytes())]);
    let mut manifest = Manifest::default();
    let done = run(&d.dir(), &[&a], false, &mut f, &mut manifest).unwrap();
    assert_eq!(
        done,
        vec![Done {
            name: "alpha".into(),
            path: d.dir().join("alpha.md"),
            outcome: Outcome::Created,
        }]
    );
    assert_eq!(fs::read_to_string(d.dir().join("alpha.md")).unwrap(), text);
    assert_eq!(
        manifest.get("alpha"),
        Some(sha256_hex(text.as_bytes()).as_str())
    );
    assert_eq!(
        fs::read_to_string(d.dir().join(".pulled.toml")).unwrap(),
        manifest.to_toml()
    );
}

#[test]
fn apply_creates_the_missing_folder() {
    let d = Dest::new();
    let text = template_text("a");
    let a = entry("alpha", &text);
    let mut f = Fetcher::new(&[("alpha", text.as_bytes())]);
    run(&d.dir(), &[&a], false, &mut f, &mut Manifest::default()).unwrap();
    assert!(d.dir().join("alpha.md").is_file());
}

#[test]
fn apply_leaves_no_temp_files_behind() {
    let d = Dest::new();
    let text = template_text("a");
    let a = entry("alpha", &text);
    let mut f = Fetcher::new(&[("alpha", text.as_bytes())]);
    run(&d.dir(), &[&a], false, &mut f, &mut Manifest::default()).unwrap();
    let mut names: Vec<String> = fs::read_dir(d.dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, [".pulled.toml", "alpha.md"]);
}

#[test]
fn apply_update_available_is_applied_without_force() {
    let d = Dest::new();
    let old = template_text("old");
    let new = template_text("new");
    d.write("alpha.md", &old);
    let a = entry("alpha", &new);
    let mut f = Fetcher::new(&[("alpha", new.as_bytes())]);
    let mut manifest = manifest_with(&[("alpha", &old)]);
    let done = run(&d.dir(), &[&a], false, &mut f, &mut manifest).unwrap();
    assert_eq!(done[0].outcome, Outcome::Updated);
    assert_eq!(fs::read_to_string(d.dir().join("alpha.md")).unwrap(), new);
    assert_eq!(
        manifest.get("alpha"),
        Some(sha256_hex(new.as_bytes()).as_str())
    );
    assert!(!d.dir().join("alpha.md.bak").exists());
}

#[test]
fn apply_modified_is_skipped_without_force() {
    let d = Dest::new();
    let pulled = template_text("pulled");
    let edited = template_text("edited");
    d.write("alpha.md", &edited);
    let a = entry("alpha", &template_text("new"));
    let mut f = Fetcher::new(&[]);
    let mut manifest = manifest_with(&[("alpha", &pulled)]);
    let before = manifest.clone();
    let done = run(&d.dir(), &[&a], false, &mut f, &mut manifest).unwrap();
    assert_eq!(done[0].outcome, Outcome::KeptModified);
    assert_eq!(
        fs::read_to_string(d.dir().join("alpha.md")).unwrap(),
        edited
    );
    assert_eq!(manifest, before);
    assert_eq!(f.calls, Vec::<String>::new());
    assert!(!d.dir().join("alpha.md.bak").exists());
}

#[test]
fn apply_up_to_date_is_not_fetched() {
    let d = Dest::new();
    let text = template_text("a");
    d.write("alpha.md", &text);
    let a = entry("alpha", &text);
    let mut f = Fetcher::new(&[]);
    let mut manifest = manifest_with(&[("alpha", &text)]);
    let done = run(&d.dir(), &[&a], false, &mut f, &mut manifest).unwrap();
    assert_eq!(done[0].outcome, Outcome::KeptUpToDate);
    assert_eq!(f.calls, Vec::<String>::new());
}

#[test]
fn apply_force_on_up_to_date_does_nothing() {
    let d = Dest::new();
    let text = template_text("a");
    d.write("alpha.md", &text);
    let a = entry("alpha", &text);
    let mut f = Fetcher::new(&[]);
    let mut manifest = manifest_with(&[("alpha", &text)]);
    let done = run(&d.dir(), &[&a], true, &mut f, &mut manifest).unwrap();
    assert_eq!(done[0].outcome, Outcome::KeptUpToDate);
    assert_eq!(f.calls, Vec::<String>::new());
    assert!(!d.dir().join("alpha.md.bak").exists());
}

#[test]
fn apply_force_saves_a_backup_then_overwrites() {
    let d = Dest::new();
    let pulled = template_text("pulled");
    let edited = template_text("edited");
    let new = template_text("new");
    d.write("alpha.md", &edited);
    let a = entry("alpha", &new);
    let mut f = Fetcher::new(&[("alpha", new.as_bytes())]);
    let mut manifest = manifest_with(&[("alpha", &pulled)]);
    let done = run(&d.dir(), &[&a], true, &mut f, &mut manifest).unwrap();
    assert_eq!(done[0].outcome, Outcome::Overwrote);
    assert_eq!(fs::read_to_string(d.dir().join("alpha.md")).unwrap(), new);
    assert_eq!(
        fs::read_to_string(d.dir().join("alpha.md.bak")).unwrap(),
        edited
    );
    assert_eq!(
        manifest.get("alpha"),
        Some(sha256_hex(new.as_bytes()).as_str())
    );
}

#[test]
fn apply_force_overwrites_a_file_minuta_never_wrote_after_backing_it_up() {
    let d = Dest::new();
    let mine = template_text("mine");
    let new = template_text("new");
    d.write("alpha.md", &mine);
    let a = entry("alpha", &new);
    let mut f = Fetcher::new(&[("alpha", new.as_bytes())]);
    let done = run(&d.dir(), &[&a], true, &mut f, &mut Manifest::default()).unwrap();
    assert_eq!(done[0].outcome, Outcome::Overwrote);
    assert_eq!(
        fs::read_to_string(d.dir().join("alpha.md.bak")).unwrap(),
        mine
    );
}

#[test]
fn apply_force_replaces_an_older_backup() {
    let d = Dest::new();
    let edited = template_text("edited");
    d.write("alpha.md", &edited);
    d.write("alpha.md.bak", "an older backup");
    let new = template_text("new");
    let a = entry("alpha", &new);
    let mut f = Fetcher::new(&[("alpha", new.as_bytes())]);
    run(&d.dir(), &[&a], true, &mut f, &mut Manifest::default()).unwrap();
    assert_eq!(
        fs::read_to_string(d.dir().join("alpha.md.bak")).unwrap(),
        edited
    );
}

#[test]
fn apply_force_backup_failure_writes_nothing() {
    let d = Dest::new();
    let edited = template_text("edited");
    d.write("alpha.md", &edited);
    let bak = d.dir().join("alpha.md.bak");
    fs::create_dir(&bak).unwrap(); // a backup cannot replace a directory
    let new_alpha = template_text("new alpha");
    let new_beta = template_text("new beta");
    let a = entry("alpha", &new_alpha);
    let b = entry("beta", &new_beta);
    let mut f = Fetcher::new(&[
        ("alpha", new_alpha.as_bytes()),
        ("beta", new_beta.as_bytes()),
    ]);
    let mut manifest = Manifest::default();
    let text = err_text(run(&d.dir(), &[&b, &a], true, &mut f, &mut manifest));
    assert!(
        text.starts_with(&format!(
            "I/O error: cannot save a backup to {}: ",
            bak.display()
        )),
        "{text}"
    );
    assert_eq!(
        fs::read_to_string(d.dir().join("alpha.md")).unwrap(),
        edited
    );
    assert!(!d.dir().join("beta.md").exists());
    assert!(!d.dir().join(".pulled.toml").exists());
    assert_eq!(manifest, Manifest::default());
}

// ---- apply: verification ----

#[test]
fn apply_hash_mismatch_writes_nothing() {
    let d = Dest::new();
    let good = template_text("good");
    let bad = template_text("tampered");
    let a = entry("alpha", &good);
    let b = entry("beta", &template_text("expected"));
    let mut f = Fetcher::new(&[("alpha", good.as_bytes()), ("beta", bad.as_bytes())]);
    let mut manifest = Manifest::default();
    let text = err_text(run(&d.dir(), &[&a, &b], false, &mut f, &mut manifest));
    assert_eq!(
        text,
        format!(
            "template beta: downloaded file does not match the catalog checksum (expected {}, got {})",
            b.sha256,
            sha256_hex(bad.as_bytes())
        )
    );
    assert!(!d.dir().join("alpha.md").exists());
    assert!(!d.dir().join("beta.md").exists());
    assert!(!d.dir().join(".pulled.toml").exists());
    assert_eq!(manifest, Manifest::default());
}

#[test]
fn apply_unparsable_template_writes_nothing() {
    let d = Dest::new();
    let good = template_text("good");
    let bad = "---\ndescription: x\nbogus: 1\n---\nBody.\n";
    let a = entry("alpha", &good);
    let b = entry("beta", bad);
    let mut f = Fetcher::new(&[("alpha", good.as_bytes()), ("beta", bad.as_bytes())]);
    let text = err_text(run(
        &d.dir(),
        &[&a, &b],
        false,
        &mut f,
        &mut Manifest::default(),
    ));
    assert!(
        text.starts_with("template beta: invalid template from the catalog: "),
        "{text}"
    );
    assert!(text.contains("bogus"), "{text}");
    assert!(
        text.ends_with("; if it uses a newer format, upgrade minuta"),
        "{text}"
    );
    assert!(!d.dir().join("alpha.md").exists());
    assert!(!d.dir().join("beta.md").exists());
}

#[test]
fn apply_file_that_is_not_utf8_writes_nothing() {
    let d = Dest::new();
    let bytes: &[u8] = &[0xff, 0xfe, 0x00];
    let mut a = entry("alpha", "x");
    a.sha256 = sha256_hex(bytes); // the hash matches, so only the encoding is wrong
    let mut f = Fetcher::new(&[("alpha", bytes)]);
    assert_eq!(
        err_text(run(
            &d.dir(),
            &[&a],
            false,
            &mut f,
            &mut Manifest::default()
        )),
        "template alpha: downloaded file is not valid UTF-8"
    );
    assert!(!d.dir().join("alpha.md").exists());
}

#[test]
fn apply_fetch_error_writes_nothing() {
    let d = Dest::new();
    let good = template_text("good");
    let a = entry("alpha", &good);
    let b = entry("beta", &template_text("missing on the server"));
    let mut f = Fetcher::new(&[("alpha", good.as_bytes())]); // beta has no file
    let text = err_text(run(
        &d.dir(),
        &[&a, &b],
        false,
        &mut f,
        &mut Manifest::default(),
    ));
    assert_eq!(text, "I/O error: no fake file for beta");
    assert!(!d.dir().join("alpha.md").exists());
}

#[test]
fn apply_only_fetches_what_it_will_write() {
    let d = Dest::new();
    let kept = template_text("kept");
    d.write("kept.md", &kept);
    let fresh = template_text("fresh");
    let k = entry("kept", &kept);
    let n = entry("fresh", &fresh);
    let mut f = Fetcher::new(&[("fresh", fresh.as_bytes())]);
    let mut manifest = manifest_with(&[("kept", &kept)]);
    run(&d.dir(), &[&k, &n], false, &mut f, &mut manifest).unwrap();
    assert_eq!(f.calls, ["fresh"]);
}

// ---- apply: the manifest ----

#[test]
fn apply_manifest_changes_only_for_files_written() {
    let d = Dest::new();
    let other = template_text("other");
    let pulled = template_text("pulled");
    let edited = template_text("edited");
    d.write("edited.md", &edited);
    let fresh = template_text("fresh");
    let e_edited = entry("edited", &template_text("upstream"));
    let e_fresh = entry("fresh", &fresh);
    let mut f = Fetcher::new(&[("fresh", fresh.as_bytes())]);
    let mut manifest = manifest_with(&[("other", &other), ("edited", &pulled)]);
    run(
        &d.dir(),
        &[&e_edited, &e_fresh],
        false,
        &mut f,
        &mut manifest,
    )
    .unwrap();
    assert_eq!(
        manifest,
        manifest_with(&[("other", &other), ("edited", &pulled), ("fresh", &fresh)])
    );
    assert_eq!(
        fs::read_to_string(d.dir().join(".pulled.toml")).unwrap(),
        manifest.to_toml()
    );
}

#[test]
fn apply_nothing_written_leaves_the_manifest_file_alone() {
    let d = Dest::new();
    let text = template_text("a");
    d.write("alpha.md", &text);
    let m = manifest_with(&[("alpha", &text)]);
    d.write(".pulled.toml", "# my own comment\nversion = 1\n");
    let a = entry("alpha", &text);
    let mut f = Fetcher::new(&[]);
    let mut manifest = m.clone();
    run(&d.dir(), &[&a], false, &mut f, &mut manifest).unwrap();
    assert_eq!(
        fs::read_to_string(d.dir().join(".pulled.toml")).unwrap(),
        "# my own comment\nversion = 1\n"
    );
}

#[test]
fn an_invalid_manifest_makes_every_file_modified_and_nothing_is_overwritten() {
    let d = Dest::new();
    let mine = template_text("mine");
    d.write("alpha.md", &mine);
    d.write(".pulled.toml", "version = [");
    let a = entry("alpha", &template_text("upstream"));
    let (mut manifest, warning) = read_manifest(&d.dir()).unwrap();
    assert!(warning.is_some());
    let mut f = Fetcher::new(&[]);
    let done = run(&d.dir(), &[&a], false, &mut f, &mut manifest).unwrap();
    assert_eq!(done[0].outcome, Outcome::KeptModified);
    assert_eq!(fs::read_to_string(d.dir().join("alpha.md")).unwrap(), mine);
}

#[test]
fn apply_write_failure_keeps_the_manifest_truthful() {
    let d = Dest::new();
    let dir = d.create();
    let first = template_text("first");
    let second = template_text("second");
    // A directory where the temp file goes makes the second write fail after the first
    // one has landed.
    fs::create_dir(dir.join("second.md.tmp")).unwrap();
    let a = entry("first", &first);
    let b = entry("second", &second);
    let mut f = Fetcher::new(&[("first", first.as_bytes()), ("second", second.as_bytes())]);
    let mut manifest = Manifest::default();
    let text = err_text(run(&dir, &[&a, &b], false, &mut f, &mut manifest));
    assert!(
        text.starts_with(&format!(
            "I/O error: cannot write {}: ",
            dir.join("second.md").display()
        )),
        "{text}"
    );
    assert_eq!(fs::read_to_string(dir.join("first.md")).unwrap(), first);
    assert!(!dir.join("second.md").exists());
    assert_eq!(manifest, manifest_with(&[("first", &first)]));
    assert_eq!(
        fs::read_to_string(dir.join(".pulled.toml")).unwrap(),
        manifest.to_toml()
    );
}

// ---- apply: symlinks planted by an untrusted folder ----

#[cfg(unix)]
#[test]
fn apply_does_not_write_through_a_planted_temp_symlink() {
    let d = Dest::new();
    let dir = d.create();
    let victim = d.outside("victim.txt", "do not touch");
    std::os::unix::fs::symlink(&victim, dir.join("alpha.md.tmp")).unwrap();
    let text = template_text("a");
    let a = entry("alpha", &text);
    let mut f = Fetcher::new(&[("alpha", text.as_bytes())]);
    run(&dir, &[&a], false, &mut f, &mut Manifest::default()).unwrap();
    assert_eq!(fs::read_to_string(&victim).unwrap(), "do not touch");
    assert_eq!(fs::read_to_string(dir.join("alpha.md")).unwrap(), text);
    assert!(!dir.join("alpha.md").is_symlink());
}

#[cfg(unix)]
#[test]
fn apply_does_not_write_the_manifest_through_a_planted_temp_symlink() {
    let d = Dest::new();
    let dir = d.create();
    let victim = d.outside("victim.txt", "do not touch");
    std::os::unix::fs::symlink(&victim, dir.join(".pulled.toml.tmp")).unwrap();
    let text = template_text("a");
    let a = entry("alpha", &text);
    let mut f = Fetcher::new(&[("alpha", text.as_bytes())]);
    let mut manifest = Manifest::default();
    run(&dir, &[&a], false, &mut f, &mut manifest).unwrap();
    assert_eq!(fs::read_to_string(&victim).unwrap(), "do not touch");
    assert_eq!(
        fs::read_to_string(dir.join(".pulled.toml")).unwrap(),
        manifest.to_toml()
    );
}

#[cfg(unix)]
#[test]
fn apply_does_not_write_a_backup_through_a_planted_temp_symlink() {
    let d = Dest::new();
    let dir = d.create();
    let edited = template_text("edited");
    d.write("alpha.md", &edited);
    let victim = d.outside("victim.txt", "do not touch");
    std::os::unix::fs::symlink(&victim, dir.join("alpha.md.bak.tmp")).unwrap();
    let new = template_text("new");
    let a = entry("alpha", &new);
    let mut f = Fetcher::new(&[("alpha", new.as_bytes())]);
    run(&dir, &[&a], true, &mut f, &mut Manifest::default()).unwrap();
    assert_eq!(fs::read_to_string(&victim).unwrap(), "do not touch");
    assert_eq!(
        fs::read_to_string(dir.join("alpha.md.bak")).unwrap(),
        edited
    );
}

// ---- end to end with a server ----

#[test]
fn pull_end_to_end_against_a_server() {
    let d = Dest::new();
    let ta = template_text("a");
    let tb = template_text("b");
    let a = entry("alpha", &ta);
    let b = entry("beta", &tb);
    let server = FakeHttp::start(vec![
        Route::ok("/templates/index.toml", catalog_text(&[&a, &b])),
        Route::ok("/templates/alpha.md", ta.clone()),
        Route::ok("/templates/beta.md", tb.clone()),
    ]);
    let origin = Origin::new(&server.base()).unwrap();
    let catalog: Catalog = fetch_catalog(&origin).unwrap();
    let selection: Vec<&Entry> = catalog.entries.iter().collect();
    let (mut manifest, warning) = read_manifest(&d.dir()).unwrap();
    assert_eq!(warning, None);
    let items = plan(&d.dir(), &selection, &manifest).unwrap();
    let done = apply(
        &d.dir(),
        &items,
        false,
        &mut |e| fetch_file(&origin, e),
        &mut manifest,
    )
    .unwrap();
    let outcomes: Vec<Outcome> = done.iter().map(|x| x.outcome).collect();
    assert_eq!(outcomes, [Outcome::Created, Outcome::Created]);
    assert_eq!(fs::read_to_string(d.dir().join("alpha.md")).unwrap(), ta);
    assert_eq!(fs::read_to_string(d.dir().join("beta.md")).unwrap(), tb);
    assert_eq!(
        server.requests(),
        vec![
            "/templates/index.toml",
            "/templates/alpha.md",
            "/templates/beta.md"
        ]
    );

    // A second pull finds everything up to date and fetches no file.
    let (mut manifest, _) = read_manifest(&d.dir()).unwrap();
    let items = plan(&d.dir(), &selection, &manifest).unwrap();
    let done = apply(
        &d.dir(),
        &items,
        false,
        &mut |e| fetch_file(&origin, e),
        &mut manifest,
    )
    .unwrap();
    let outcomes: Vec<Outcome> = done.iter().map(|x| x.outcome).collect();
    assert_eq!(outcomes, [Outcome::KeptUpToDate, Outcome::KeptUpToDate]);
    assert_eq!(server.requests().len(), 3);
}
