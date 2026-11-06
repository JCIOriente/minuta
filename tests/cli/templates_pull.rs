use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use sha2::{Digest, Sha256};
use tempfile::TempDir;

#[path = "../../src/test_support/fake_http.rs"]
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
        // Lowercase hex SHA-256 of the file, as the catalog records it.
        let sha256: String = Sha256::digest(template_text(name).as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        s.push_str(&format!(
            "\n[[template]]\nname = \"{name}\"\ndescription = \"{description}\"\nsha256 = \"{sha256}\"\n"
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

#[test]
fn a_template_folder_that_cannot_be_read_does_not_block_pulling() {
    let env = Env::new();
    // beta needs a template that minuta does not have: with the check skipped, no note.
    let s = server_with(&[("beta", None, "Beta", Some("gamma"))]);
    fs::write(env.work().join("templates"), "not a folder").unwrap();
    let out = env.run(
        &["templates", "pull", "beta", "--yes", "--to", "out"],
        Some(&s.base()),
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stderr = text(&out.stderr);
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 2, "{stderr}");
    assert!(
        lines[0].starts_with("warning: cannot check which templates minuta already has: "),
        "{stderr}"
    );
    assert!(
        lines[0].ends_with("; notes about missing input templates are skipped"),
        "{stderr}"
    );
    assert_eq!(lines[1], "beta.md  written");
    assert!(env.work().join("out/beta.md").is_file());
}
