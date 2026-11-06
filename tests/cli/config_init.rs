use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use crate::common::{output, stderr, stdout};

/// A temp folder with an empty `home/`; the binary sees only `HOME` (and `MINUTA_CONFIG`
/// when a test sets it). Unlike `common::Env`, it sets no `MINUTA_CONFIG`, so `config init`
/// writes to its default path under `HOME`.
struct Env {
    tmp: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("home")).unwrap();
        Env { tmp }
    }

    fn home(&self) -> PathBuf {
        self.tmp.path().join("home")
    }

    fn default_config(&self) -> PathBuf {
        self.home().join(".minuta/config.toml")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(assert_cmd::cargo::cargo_bin("minuta"));
        cmd.args(args)
            .current_dir(self.tmp.path())
            .env_clear()
            .env("HOME", self.home())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    fn minuta(&self, args: &[&str]) -> Output {
        output(&mut self.command(args))
    }
}

fn assert_ok(out: &Output) {
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(out));
}

/// The exact file `minuta config init` writes. `config_init::tests` pins `TEMPLATE` to it.
fn template_fixture() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/config_init/config.toml"
    ))
    .unwrap()
}

/// The exact `config show` output after a fresh `config init`. `config_init::tests` pins the
/// library's defaults to it.
fn show_fixture() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/config_init/show.txt"
    ))
    .unwrap()
}

#[test]
fn config_init_creates_file_and_parent() {
    let env = Env::new();
    let path = env.default_config();
    assert!(!path.parent().unwrap().try_exists().unwrap());

    let out = env.minuta(&["config", "init"]);
    assert_ok(&out);
    assert_eq!(stdout(&out), format!("created {}\n", path.display()));
    assert_eq!(stderr(&out), "");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), template_fixture());

    // `config show` then reports only defaults and unset keys.
    let show = env.minuta(&["config", "show"]);
    assert_ok(&show);
    assert_eq!(stdout(&show), show_fixture());
}

#[test]
fn config_init_leaves_existing_file() {
    let env = Env::new();
    let path = env.default_config();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "out_dir = \"/mine\"\n").unwrap();

    let out = env.minuta(&["config", "init"]);
    assert_ok(&out);
    assert_eq!(
        stdout(&out),
        format!(
            "{} already exists; nothing changed (use --force to overwrite)\n",
            path.display()
        )
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "out_dir = \"/mine\"\n"
    );
}

#[test]
fn config_init_second_run_leaves_template() {
    let env = Env::new();
    let path = env.default_config();
    assert_ok(&env.minuta(&["config", "init"]));
    let out = env.minuta(&["config", "init"]);
    assert_ok(&out);
    assert_eq!(
        stdout(&out),
        format!(
            "{} already exists; nothing changed (use --force to overwrite)\n",
            path.display()
        )
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), template_fixture());
}

#[test]
fn config_init_force_overwrites() {
    let env = Env::new();
    let path = env.default_config();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "out_dir = \"/mine\"\n# a much longer line than the template has at the end of it, so truncation shows\n".repeat(200)).unwrap();

    let out = env.minuta(&["config", "init", "--force"]);
    assert_ok(&out);
    assert_eq!(stdout(&out), format!("overwrote {}\n", path.display()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), template_fixture());
}

#[test]
fn config_init_force_creates_missing_file() {
    let env = Env::new();
    let path = env.default_config();
    let out = env.minuta(&["config", "init", "--force"]);
    assert_ok(&out);
    assert_eq!(stdout(&out), format!("created {}\n", path.display()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), template_fixture());
}

#[test]
fn config_init_ignores_malformed_existing_file() {
    let env = Env::new();
    let path = env.default_config();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "this is [not toml\n").unwrap();

    let out = env.minuta(&["config", "init"]);
    assert_ok(&out);
    assert_eq!(
        stdout(&out),
        format!(
            "{} already exists; nothing changed (use --force to overwrite)\n",
            path.display()
        )
    );
    assert_eq!(stderr(&out), "");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "this is [not toml\n"
    );
}

#[test]
fn config_init_honours_minuta_config() {
    let env = Env::new();
    let custom = env.tmp.path().join("elsewhere/nested/minuta.toml");
    let mut cmd = env.command(&["config", "init"]);
    cmd.env("MINUTA_CONFIG", &custom);
    let out = output(&mut cmd);
    assert_ok(&out);
    assert_eq!(stdout(&out), format!("created {}\n", custom.display()));
    assert_eq!(
        std::fs::read_to_string(&custom).unwrap(),
        template_fixture()
    );
    assert!(!env.default_config().try_exists().unwrap());
    assert!(!env.home().join(".minuta").try_exists().unwrap());
}

#[test]
fn config_init_io_error_names_path() {
    let env = Env::new();
    // The parent "folder" is a regular file, so the folder cannot be created.
    let blocker = env.tmp.path().join("blocker");
    std::fs::write(&blocker, "").unwrap();
    let target = blocker.join("config.toml");
    let mut cmd = env.command(&["config", "init"]);
    cmd.env("MINUTA_CONFIG", &target);
    let out = output(&mut cmd);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    let source = std::fs::create_dir_all(&blocker).unwrap_err();
    assert_eq!(
        stderr(&out),
        format!("minuta: config error: {}: {source}\n", blocker.display())
    );
}
