use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use minuta::error::{Error, Result};
use minuta::template::catalog::{sha256_hex, Manifest};
use minuta::template::pull::Origin;
use minuta::template::pull_cmd::{parse_choice, run, Choice, Options, Terminal};
use tempfile::TempDir;

#[path = "support/fake_http.rs"]
mod fake_http;

use fake_http::{FakeHttp, Route};

// ---- fixtures ----

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
    known: Vec<String>,
}

fn case(choice: Choice) -> Case {
    Case {
        choice,
        list: false,
        force: false,
        yes: false,
        interactive: false,
        input: "",
        known: vec![],
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
            known: &self.known,
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

// ---- --list ----

#[test]
fn list_prints_the_table_and_downloads_nothing() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Ask);
    c.list = true;
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            "templates from {}/\n\
NAME   PLATFORM  STATE  DESCRIPTION\n\
alpha  social    new    Alpha post\n\
beta   -         new    Beta notes\n\
gamma  audio     new    Gamma audio\n",
            s.base()
        )
    );
    assert_eq!(out.stdout, "");
    assert_eq!(s.requests(), vec!["/templates/index.toml"]);
    assert!(!d.dir().exists());
}

#[test]
fn list_shows_the_state_of_each_template() {
    let d = Dest::new();
    d.pulled("alpha", &template_text("alpha"));
    d.pulled("beta", "an older beta\n");
    d.write("gamma.md", "my own gamma\n");
    let s = server();
    let mut c = case(Choice::Ask);
    c.list = true;
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            "templates from {}/\n\
NAME   PLATFORM  STATE             DESCRIPTION\n\
alpha  social    up to date        Alpha post\n\
beta   -         update available  Beta notes\n\
gamma  audio     modified          Gamma audio\n",
            s.base()
        )
    );
}

#[test]
fn list_needs_no_terminal_and_no_yes() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Ask);
    c.list = true;
    c.interactive = false;
    c.yes = false;
    c.run(&s, &d.dir()).result.unwrap();
}

#[test]
fn list_escapes_control_characters_from_the_catalog() {
    let d = Dest::new();
    let s = server_with(&[("alpha", Some("so\\u001bcial"), "bad\\u001b[2Jtext", None)]);
    let mut c = case(Choice::Ask);
    c.list = true;
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            "templates from {}/\n\
NAME   PLATFORM  STATE  DESCRIPTION\n\
alpha  so?cial   new    bad?[2Jtext\n",
            s.base()
        )
    );
}

// ---- choosing ----

#[test]
fn by_name_with_yes_writes_the_files_and_prints_only_the_paths() {
    let d = Dest::new();
    let s = server();
    let mut c = case(names(&["beta", "alpha"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stdout,
        format!(
            "{}\n{}\n",
            d.dir().join("beta.md").display(),
            d.dir().join("alpha.md").display()
        )
    );
    assert_eq!(out.stderr, "beta.md   written\nalpha.md  written\n");
    assert_eq!(d.read("alpha.md"), template_text("alpha"));
    assert_eq!(d.read("beta.md"), template_text("beta"));
    assert!(!d.dir().join("gamma.md").exists());
    assert_eq!(
        s.requests(),
        vec![
            "/templates/index.toml",
            "/templates/beta.md",
            "/templates/alpha.md"
        ]
    );
}

#[test]
fn all_with_yes_writes_every_template() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::All);
    c.yes = true;
    c.run(&s, &d.dir()).result.unwrap();
    for name in ["alpha", "beta", "gamma"] {
        assert_eq!(d.read(&format!("{name}.md")), template_text(name), "{name}");
    }
}

#[test]
fn filter_with_yes_writes_the_matching_templates() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Filter("AUDIO".into()));
    c.yes = true;
    c.run(&s, &d.dir()).result.unwrap();
    assert!(d.dir().join("gamma.md").is_file());
    assert!(!d.dir().join("alpha.md").exists());
    assert!(!d.dir().join("beta.md").exists());
}

#[test]
fn filter_without_a_match_is_a_usage_error_and_downloads_no_file() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Filter("zzz".into()));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    assert_eq!(usage(&out.result), "no template matches filter \"zzz\"");
    assert_eq!(s.requests(), vec!["/templates/index.toml"]);
    assert!(!d.dir().exists());
}

#[test]
fn an_unknown_name_lists_the_available_ones() {
    let d = Dest::new();
    let s = server();
    let mut c = case(names(&["alpha", "nope"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    assert_eq!(
        usage(&out.result),
        "unknown template \"nope\" (available: alpha, beta, gamma)"
    );
    assert!(!d.dir().exists());
}

// ---- refusing ----

#[test]
fn without_a_terminal_and_without_yes_it_refuses_before_any_request() {
    let d = Dest::new();
    let s = server();
    let c = case(names(&["alpha"]));
    let out = c.run(&s, &d.dir());
    assert_eq!(
        usage(&out.result),
        "minuta asks before it downloads: pass --yes, or run it from a terminal"
    );
    assert_eq!(s.requests(), Vec::<String>::new());
    assert!(!d.dir().exists());
    assert_eq!((out.stdout.as_str(), out.stderr.as_str()), ("", ""));
}

#[test]
fn without_a_terminal_it_cannot_open_the_list() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Ask);
    c.yes = true;
    let out = c.run(&s, &d.dir());
    assert_eq!(
        usage(&out.result),
        "nothing selected: name the templates to download, or use --all or --filter TEXT (a list opens only on a terminal)"
    );
    assert_eq!(s.requests(), Vec::<String>::new());
}

// ---- the prompt ----

#[test]
fn the_prompt_names_the_source_the_files_and_the_destination() {
    let d = Dest::new();
    let s = server();
    let mut c = case(names(&["alpha", "beta"]));
    c.interactive = true;
    c.input = "y\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            "minuta will download from:\n  {}/\n  alpha.md  (new)\n  beta.md   (new)\ninto {}\nContinue? [y/N] alpha.md  written\nbeta.md   written\n",
            s.base(),
            d.dir().display()
        )
    );
    assert_eq!(
        out.stdout,
        format!(
            "{}\n{}\n",
            d.dir().join("alpha.md").display(),
            d.dir().join("beta.md").display()
        )
    );
}

#[test]
fn yes_and_yes_in_words_both_confirm() {
    for answer in ["y\n", "Y\n", "yes\n", "YES\n", "  yes  \n"] {
        let d = Dest::new();
        let s = server();
        let mut c = case(names(&["alpha"]));
        c.interactive = true;
        c.input = answer;
        c.run(&s, &d.dir()).result.unwrap();
        assert!(d.dir().join("alpha.md").is_file(), "answer {answer:?}");
    }
}

#[test]
fn any_other_answer_cancels_and_downloads_no_file() {
    for answer in ["n\n", "\n", "", "no\n", "yy\n", "ok\n"] {
        let d = Dest::new();
        let s = server();
        let mut c = case(names(&["alpha"]));
        c.interactive = true;
        c.input = answer;
        let out = c.run(&s, &d.dir());
        out.result.unwrap();
        assert_eq!(
            out.stderr,
            format!(
                "minuta will download from:\n  {}/\n  alpha.md  (new)\ninto {}\nContinue? [y/N] cancelled\n",
                s.base(),
                d.dir().display()
            ),
            "answer {answer:?}"
        );
        assert_eq!(out.stdout, "", "answer {answer:?}");
        assert_eq!(s.requests(), vec!["/templates/index.toml"], "{answer:?}");
        assert!(!d.dir().exists(), "answer {answer:?}");
    }
}

#[test]
fn yes_skips_the_prompt_even_on_a_terminal() {
    let d = Dest::new();
    let s = server();
    let mut c = case(names(&["alpha"]));
    c.interactive = true;
    c.yes = true;
    c.input = "n\n"; // would cancel if it were read
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(out.stderr, "alpha.md  written\n");
    assert!(d.dir().join("alpha.md").is_file());
}

#[test]
fn when_there_is_nothing_to_download_it_does_not_ask() {
    let d = Dest::new();
    d.pulled("alpha", &template_text("alpha"));
    let s = server();
    let mut c = case(names(&["alpha"]));
    c.interactive = true;
    c.input = "n\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(out.stderr, "alpha.md  up to date\nnothing to download\n");
    assert_eq!(out.stdout, "");
    assert_eq!(s.requests(), vec!["/templates/index.toml"]);
}

#[test]
fn a_modified_file_is_listed_as_skipped_and_kept_without_force() {
    let d = Dest::new();
    d.pulled("beta", "an older beta\n");
    d.write("beta.md", "my edited beta\n");
    let s = server();
    let mut c = case(names(&["alpha", "beta"]));
    c.interactive = true;
    c.input = "y\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            "minuta will download from:\n  {}/\n  alpha.md  (new)\n  beta.md   (modified: skipped, use --force to overwrite)\ninto {}\nContinue? [y/N] alpha.md  written\nbeta.md   skipped (modified; use --force to overwrite)\n",
            s.base(),
            d.dir().display()
        )
    );
    assert_eq!(
        out.stdout,
        format!("{}\n", d.dir().join("alpha.md").display())
    );
    assert_eq!(d.read("beta.md"), "my edited beta\n");
    assert!(!d.dir().join("beta.md.bak").exists());
}

#[test]
fn an_update_is_offered_without_force() {
    let d = Dest::new();
    d.pulled("beta", "an older beta\n");
    let s = server();
    let mut c = case(names(&["beta"]));
    c.interactive = true;
    c.input = "y\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            "minuta will download from:\n  {}/\n  beta.md  (update available)\ninto {}\nContinue? [y/N] beta.md  updated\n",
            s.base(),
            d.dir().display()
        )
    );
    assert_eq!(d.read("beta.md"), template_text("beta"));
}

#[test]
fn force_replaces_a_modified_file_and_says_where_the_backup_is() {
    let d = Dest::new();
    d.pulled("beta", "an older beta\n");
    d.write("beta.md", "my edited beta\n");
    let s = server();
    let mut c = case(names(&["beta"]));
    c.force = true;
    c.interactive = true;
    c.input = "y\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            "minuta will download from:\n  {}/\n  beta.md  (modified: replaced, old file saved as beta.md.bak)\ninto {}\nContinue? [y/N] beta.md  overwritten (backup: beta.md.bak)\n",
            s.base(),
            d.dir().display()
        )
    );
    assert_eq!(d.read("beta.md"), template_text("beta"));
    assert_eq!(d.read("beta.md.bak"), "my edited beta\n");
    assert_eq!(
        out.stdout,
        format!("{}\n", d.dir().join("beta.md").display())
    );
}

#[test]
fn the_manifest_records_what_was_written() {
    let d = Dest::new();
    let s = server();
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    c.run(&s, &d.dir()).result.unwrap();
    let (manifest, warning) = Manifest::parse(&d.read(".pulled.toml"));
    assert_eq!(warning, None);
    assert_eq!(manifest.get("alpha"), Some(sha("alpha").as_str()));
}

#[test]
fn a_manifest_that_cannot_be_trusted_is_reported_and_nothing_is_overwritten() {
    let d = Dest::new();
    d.write("alpha.md", "my alpha\n");
    d.write(".pulled.toml", "version = [");
    let s = server();
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert!(
        out.stderr
            .starts_with("warning: invalid manifest, treating every file as modified: "),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr.ends_with(
            "alpha.md  skipped (modified; use --force to overwrite)\nnothing to download\n"
        ),
        "{}",
        out.stderr
    );
    assert_eq!(d.read("alpha.md"), "my alpha\n");
}

// ---- the numbered list ----

const LIST: &str =
    " 1) alpha  new  Alpha post\n 2) beta   new  Beta notes\n 3) gamma  new  Gamma audio\n";
const ASK: &str =
    "Select templates (numbers separated by commas or spaces, a for all, empty to cancel): ";

#[test]
fn the_list_selects_by_number() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Ask);
    c.interactive = true;
    c.input = "1,3\ny\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            "{LIST}{ASK}minuta will download from:\n  {}/\n  alpha.md  (new)\n  gamma.md  (new)\ninto {}\nContinue? [y/N] alpha.md  written\ngamma.md  written\n",
            s.base(),
            d.dir().display()
        )
    );
    assert!(d.dir().join("alpha.md").is_file());
    assert!(!d.dir().join("beta.md").exists());
    assert!(d.dir().join("gamma.md").is_file());
}

#[test]
fn the_list_selects_everything_with_a() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Ask);
    c.interactive = true;
    c.input = "a\nyes\n";
    c.run(&s, &d.dir()).result.unwrap();
    for name in ["alpha", "beta", "gamma"] {
        assert!(d.dir().join(format!("{name}.md")).is_file(), "{name}");
    }
}

#[test]
fn an_empty_answer_to_the_list_cancels() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Ask);
    c.interactive = true;
    c.input = "\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(out.stderr, format!("{LIST}{ASK}nothing selected\n"));
    assert_eq!(s.requests(), vec!["/templates/index.toml"]);
    assert!(!d.dir().exists());
}

#[test]
fn an_invalid_answer_to_the_list_is_a_usage_error() {
    let d = Dest::new();
    let s = server();
    let mut c = case(Choice::Ask);
    c.interactive = true;
    c.input = "x\n";
    let out = c.run(&s, &d.dir());
    assert_eq!(
        usage(&out.result),
        "invalid choice \"x\" (enter numbers from 1 to 3, 'a' for all, or nothing to cancel)"
    );
    assert_eq!(s.requests(), vec!["/templates/index.toml"]);
}

#[test]
fn the_list_shows_the_state_of_each_template() {
    let d = Dest::new();
    d.pulled("alpha", &template_text("alpha"));
    d.pulled("beta", "an older beta\n");
    let s = server();
    let mut c = case(Choice::Ask);
    c.interactive = true;
    c.input = "\n";
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(
        out.stderr,
        format!(
            " 1) alpha  up to date        Alpha post\n 2) beta   update available  Beta notes\n 3) gamma  new               Gamma audio\n{ASK}nothing selected\n"
        )
    );
}

#[test]
fn parse_choice_accepts_numbers_in_the_order_given() {
    assert_eq!(parse_choice("1", 3).unwrap(), vec![0]);
    assert_eq!(parse_choice("1,3", 3).unwrap(), vec![0, 2]);
    assert_eq!(parse_choice("3 1", 3).unwrap(), vec![2, 0]);
    assert_eq!(parse_choice(" 2 , 3 ", 3).unwrap(), vec![1, 2]);
}

#[test]
fn parse_choice_drops_repeats() {
    assert_eq!(parse_choice("1 1,1", 3).unwrap(), vec![0]);
}

#[test]
fn parse_choice_a_means_all() {
    for text in ["a", "A", "all", "All", " a "] {
        assert_eq!(parse_choice(text, 3).unwrap(), vec![0, 1, 2], "{text:?}");
    }
}

#[test]
fn parse_choice_nothing_means_nothing() {
    for text in ["", "  ", "\t", ",", " , "] {
        assert_eq!(
            parse_choice(text, 3).unwrap(),
            Vec::<usize>::new(),
            "{text:?}"
        );
    }
}

#[test]
fn parse_choice_rejects_anything_else() {
    for (text, bad) in [
        ("0", "0"),
        ("4", "4"),
        ("x", "x"),
        ("1,x", "x"),
        ("-1", "-1"),
        ("1-3", "1-3"),
        ("a 2", "a"),
        ("1.5", "1.5"),
    ] {
        match parse_choice(text, 3) {
            Err(Error::Usage(m)) => assert_eq!(
                m,
                format!("invalid choice {bad:?} (enter numbers from 1 to 3, 'a' for all, or nothing to cancel)"),
                "{text:?}"
            ),
            other => panic!("{text:?}: expected usage error, got {other:?}"),
        }
    }
}

// ---- a missing input template ----

fn chain_server() -> FakeHttp {
    server_with(&[
        ("alpha", None, "Alpha post", Some("gamma -> transcript")),
        ("gamma", None, "Gamma", None),
    ])
}

const NOTE: &str =
    "note: alpha needs template gamma, which is not available (pull it too or add it to your templates)\n";

#[test]
fn a_missing_input_template_is_noted() {
    let d = Dest::new();
    let s = chain_server();
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    out.result.unwrap();
    assert_eq!(out.stderr, format!("alpha.md  written\n{NOTE}"));
}

#[test]
fn no_note_when_minuta_already_has_the_input_template() {
    let d = Dest::new();
    let s = chain_server();
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    c.known = vec!["gamma".to_string()];
    let out = c.run(&s, &d.dir());
    assert_eq!(out.stderr, "alpha.md  written\n");
}

#[test]
fn no_note_when_the_input_template_is_pulled_too() {
    let d = Dest::new();
    let s = chain_server();
    let mut c = case(names(&["alpha", "gamma"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    assert_eq!(out.stderr, "alpha.md  written\ngamma.md  written\n");
}

#[test]
fn transcript_is_never_a_missing_input() {
    let d = Dest::new();
    let s = server_with(&[("alpha", None, "Alpha", Some("transcript"))]);
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    assert_eq!(c.run(&s, &d.dir()).stderr, "alpha.md  written\n");
}

#[test]
fn the_note_escapes_control_characters_from_the_catalog() {
    let d = Dest::new();
    let s = server_with(&[("alpha", None, "Alpha", Some("bad\\u001b[2J -> transcript"))]);
    let mut c = case(names(&["alpha"]));
    c.yes = true;
    let out = c.run(&s, &d.dir());
    assert_eq!(
        out.stderr,
        "alpha.md  written\nnote: alpha needs template bad?[2J, which is not available (pull it too or add it to your templates)\n"
    );
}

// ---- the binary ----

struct Env {
    tmp: TempDir,
}

impl Env {
    fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("home")).unwrap();
        fs::create_dir(tmp.path().join("work")).unwrap();
        Env { tmp }
    }

    /// The working folder, as the binary sees it (macOS temp folders are symlinks).
    fn work(&self) -> PathBuf {
        fs::canonicalize(self.tmp.path().join("work")).unwrap()
    }

    fn command(&self, args: &[&str], url: Option<&str>) -> Command {
        let mut cmd = Command::new(assert_cmd::cargo::cargo_bin("minuta"));
        cmd.env_clear()
            .env("HOME", self.tmp.path().join("home"))
            .current_dir(self.work())
            .args(args)
            .stdin(Stdio::null());
        if let Some(url) = url {
            cmd.env("MINUTA_TEMPLATES_URL", url);
        }
        cmd
    }

    fn run(&self, args: &[&str], url: Option<&str>) -> Output {
        self.command(args, url).output().unwrap()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[test]
fn help_lists_every_flag() {
    let env = Env::new();
    let out = env.run(&["templates", "pull", "--help"], None);
    assert!(out.status.success());
    let help = text(&out.stdout);
    for flag in [
        "--all", "--filter", "--list", "--to", "--force", "--yes", "--ref",
    ] {
        assert!(help.contains(flag), "{flag} missing from:\n{help}");
    }
}

#[test]
fn conflicting_flags_are_rejected() {
    let env = Env::new();
    for args in [
        &["templates", "pull", "alpha", "--all"][..],
        &["templates", "pull", "alpha", "--filter", "x"][..],
        &["templates", "pull", "--all", "--filter", "x"][..],
        &["templates", "pull", "--list", "--force"][..],
        &["templates", "pull", "--list", "--all"][..],
        &["templates", "pull", "--list", "--filter", "x"][..],
        &["templates", "pull", "--list", "alpha"][..],
    ] {
        let out = env.run(args, None);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(
            text(&out.stderr).contains("cannot be used with"),
            "{args:?}: {}",
            text(&out.stderr)
        );
    }
}

#[test]
fn list_prints_the_table_to_stderr() {
    let env = Env::new();
    let s = server();
    let out = env.run(&["templates", "pull", "--list"], Some(&s.base()));
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out.stdout), "");
    assert_eq!(
        text(&out.stderr),
        format!(
            "templates from {}/\n\
NAME   PLATFORM  STATE  DESCRIPTION\n\
alpha  social    new    Alpha post\n\
beta   -         new    Beta notes\n\
gamma  audio     new    Gamma audio\n",
            s.base()
        )
    );
    assert!(!env.work().join("templates").exists());
}

#[test]
fn pull_by_name_with_yes_prints_the_path_and_writes_into_the_working_folder() {
    let env = Env::new();
    let s = server();
    let out = env.run(&["templates", "pull", "alpha", "--yes"], Some(&s.base()));
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let file = env.work().join("templates/alpha.md");
    assert_eq!(text(&out.stdout), format!("{}\n", file.display()));
    assert_eq!(text(&out.stderr), "alpha.md  written\n");
    assert_eq!(fs::read_to_string(&file).unwrap(), template_text("alpha"));
}

#[test]
fn a_second_pull_finds_everything_up_to_date() {
    let env = Env::new();
    let s = server();
    let args = ["templates", "pull", "alpha", "--yes"];
    env.run(&args, Some(&s.base()));
    let out = env.run(&args, Some(&s.base()));
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out.stdout), "");
    assert_eq!(
        text(&out.stderr),
        "alpha.md  up to date\nnothing to download\n"
    );
}

#[test]
fn a_pulled_template_is_listed_with_the_project_layer() {
    let env = Env::new();
    let s = server();
    env.run(&["templates", "pull", "alpha", "--yes"], Some(&s.base()));
    let out = env.run(&["templates", "list"], None);
    assert_eq!(out.status.code(), Some(0));
    let listing = text(&out.stdout);
    let line = listing
        .lines()
        .find(|l| l.starts_with("alpha "))
        .unwrap_or_else(|| panic!("no alpha row in:\n{listing}"));
    assert!(line.contains("  project  "), "{line}");
}

#[test]
fn the_manifest_and_backup_files_are_not_listed_as_invalid() {
    let env = Env::new();
    let s = server();
    env.run(&["templates", "pull", "alpha", "--yes"], Some(&s.base()));
    let file = env.work().join("templates/alpha.md");
    fs::write(&file, "my own alpha\n").unwrap();
    env.run(
        &["templates", "pull", "alpha", "--yes", "--force"],
        Some(&s.base()),
    );
    assert!(env.work().join("templates/alpha.md.bak").is_file());
    assert!(env.work().join("templates/.pulled.toml").is_file());
    let out = env.run(&["templates", "list"], None);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        !text(&out.stdout).contains("invalid"),
        "{}",
        text(&out.stdout)
    );
}

#[test]
fn to_puts_the_files_in_another_folder() {
    let env = Env::new();
    let s = server();
    let out = env.run(
        &["templates", "pull", "beta", "--yes", "--to", "elsewhere"],
        Some(&s.base()),
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let file = env.work().join("elsewhere/beta.md");
    assert_eq!(text(&out.stdout), format!("{}\n", file.display()));
    assert!(file.is_file());
    assert!(!env.work().join("templates").exists());
}

#[test]
fn without_a_terminal_and_without_yes_it_refuses_and_makes_no_request() {
    let env = Env::new();
    let s = server();
    let out = env.run(&["templates", "pull", "alpha"], Some(&s.base()));
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(text(&out.stdout), "");
    assert_eq!(
        text(&out.stderr),
        "minuta: usage error: minuta asks before it downloads: pass --yes, or run it from a terminal\n"
    );
    assert_eq!(s.requests(), Vec::<String>::new());
    assert!(!env.work().join("templates").exists());
}

#[test]
fn nothing_chosen_without_a_terminal_is_a_usage_error() {
    let env = Env::new();
    let s = server();
    let out = env.run(&["templates", "pull", "--yes"], Some(&s.base()));
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        text(&out.stderr),
        "minuta: usage error: nothing selected: name the templates to download, or use --all or --filter TEXT (a list opens only on a terminal)\n"
    );
    assert_eq!(s.requests(), Vec::<String>::new());
}

#[test]
fn an_unknown_name_is_a_usage_error() {
    let env = Env::new();
    let s = server();
    let out = env.run(&["templates", "pull", "nope", "--yes"], Some(&s.base()));
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        text(&out.stderr),
        "minuta: usage error: unknown template \"nope\" (available: alpha, beta, gamma)\n"
    );
}

#[test]
fn a_bad_ref_is_a_usage_error() {
    let env = Env::new();
    let out = env.run(
        &["templates", "pull", "alpha", "--yes", "--ref", "a b"],
        None,
    );
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        text(&out.stderr),
        "minuta: usage error: invalid ref \"a b\" (use letters, digits, '.', '_', '-' and '/')\n"
    );
}

#[test]
fn the_url_variable_takes_the_place_of_ref() {
    let env = Env::new();
    let s = server();
    let out = env.run(
        &["templates", "pull", "alpha", "--yes", "--ref", "v9.9.9"],
        Some(&s.base()),
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert_eq!(
        s.requests(),
        vec!["/templates/index.toml", "/templates/alpha.md"]
    );
}

#[test]
fn a_url_variable_that_is_not_https_is_a_usage_error() {
    let env = Env::new();
    let out = env.run(
        &["templates", "pull", "alpha", "--yes"],
        Some("http://example.com/templates"),
    );
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        text(&out.stderr),
        "minuta: usage error: templates URL \"http://example.com/templates\" must use https (http is only allowed for 127.0.0.1)\n"
    );
}

#[test]
fn control_characters_in_the_catalog_never_reach_the_terminal() {
    let env = Env::new();
    let s = server_with(&[("alpha", None, "bad\\u001b[2Jtext", None)]);
    let out = env.run(&["templates", "pull", "--list"], Some(&s.base()));
    assert_eq!(out.status.code(), Some(0));
    assert!(!text(&out.stderr).contains('\u{1b}'), "{:?}", out.stderr);
}
