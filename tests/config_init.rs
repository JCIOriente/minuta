use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use minuta::config::{Config, Overrides, Source};
use minuta::config_init::TEMPLATE;

/// A temp folder with an empty `home/`; the binary sees only `HOME` (and `MINUTA_CONFIG`
/// when a test sets it).
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
        self.command(args).output().unwrap()
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn assert_ok(out: &Output) {
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(out));
}

/// A key line of the template once uncommented: `<key> = <value>` or `[table]`.
fn is_setting(line: &str) -> bool {
    let is_key = line.split_once(" = ").is_some_and(|(key, _)| {
        !key.is_empty() && key.chars().all(|c| c.is_ascii_lowercase() || c == '_')
    });
    let is_table = line
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .is_some_and(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase()));
    is_key || is_table
}

/// The template with every commented key and table line uncommented; prose stays a comment.
fn uncommented(template: &str) -> String {
    template
        .lines()
        .map(|line| match line.strip_prefix("# ") {
            Some(rest) if is_setting(rest) => rest,
            _ => line,
        })
        .map(|line| format!("{line}\n"))
        .collect()
}

/// Every `key = value` in the template, commented or not, by dotted key name.
fn template_keys(template: &str) -> BTreeMap<String, String> {
    let mut table = String::new();
    let mut keys = BTreeMap::new();
    for line in uncommented(template).lines() {
        if let Some(name) = line.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            table = name.to_string();
        } else if let Some((key, value)) = line.split_once(" = ") {
            if !is_setting(line) {
                continue;
            }
            let dotted = if table.is_empty() {
                key.to_string()
            } else {
                format!("{table}.{key}")
            };
            keys.insert(dotted, value.to_string());
        }
    }
    keys
}

fn defaults() -> Config {
    Config::resolve(Overrides::default(), &|_| None, None).unwrap()
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
    assert_eq!(std::fs::read_to_string(&path).unwrap(), TEMPLATE);

    // `config show` then reports only defaults and unset keys.
    let show = env.minuta(&["config", "show"]);
    assert_ok(&show);
    let lines: Vec<String> = stdout(&show).lines().map(String::from).collect();
    let want: Vec<String> = defaults()
        .entries()
        .into_iter()
        .map(|(key, setting)| match setting {
            Some(s) => format!("{key} = {} (default)", s.value),
            None => format!("{key} = (unset)"),
        })
        .collect();
    assert_eq!(lines, want);
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
    assert_eq!(std::fs::read_to_string(&path).unwrap(), TEMPLATE);
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
    assert_eq!(std::fs::read_to_string(&path).unwrap(), TEMPLATE);
}

#[test]
fn config_init_force_creates_missing_file() {
    let env = Env::new();
    let path = env.default_config();
    let out = env.minuta(&["config", "init", "--force"]);
    assert_ok(&out);
    assert_eq!(stdout(&out), format!("created {}\n", path.display()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), TEMPLATE);
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
    let out = cmd.output().unwrap();
    assert_ok(&out);
    assert_eq!(stdout(&out), format!("created {}\n", custom.display()));
    assert_eq!(std::fs::read_to_string(&custom).unwrap(), TEMPLATE);
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
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    let source = std::fs::create_dir_all(&blocker).unwrap_err();
    assert_eq!(
        stderr(&out),
        format!("minuta: config error: {}: {source}\n", blocker.display())
    );
}

#[test]
fn config_init_template_covers_every_key() {
    let keys = template_keys(TEMPLATE);
    let config = defaults();
    let entries = config.entries();
    let entry_names: Vec<&str> = entries.iter().map(|(k, _)| *k).collect();
    let template_names: Vec<&str> = keys.keys().map(String::as_str).collect();
    let mut sorted = entry_names.clone();
    sorted.sort_unstable();
    assert_eq!(
        template_names, sorted,
        "template keys must match Config::entries()"
    );

    // Each key is commented out: only the `[whisper]` header is live.
    let live: Vec<&str> = TEMPLATE
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    assert_eq!(live, ["[whisper]"]);

    // Each key with a default shows exactly that default once uncommented.
    let parsed = resolve_uncommented();
    for ((key, default), (parsed_key, from_file)) in entries.iter().zip(parsed.entries()) {
        assert_eq!(*key, parsed_key);
        let from_file = from_file.unwrap_or_else(|| panic!("{key}: no value in the template"));
        assert_eq!(from_file.source, Source::File, "key {key}");
        if let Some(default) = default {
            assert_eq!(from_file.value, default.value, "key {key}");
        }
    }
}

#[test]
fn config_init_template_parses_when_uncommented() {
    let config = resolve_uncommented();
    let want: Vec<(&str, &str)> = vec![
        ("out_dir", "./minuta-out"),
        ("whisper.bin", "whisper-cli"),
        ("whisper.model", "small"),
        ("whisper.lang", "auto"),
        ("whisper.vad_model", "silero-v5.1.2"),
        ("whisper.models_dir", "~/.minuta/models"),
        ("ffmpeg.bin", "ffmpeg"),
        ("ffprobe.bin", "ffprobe"),
        ("llm.default", "ollama:gemma4:26b"),
        ("llm.output_lang", "en"),
        ("llm.concurrency", "2"),
        ("llm.retries", "3"),
        ("llm.backoff_ms", "2000"),
        ("llm.repairs", "2"),
        ("ollama.url", "http://127.0.0.1:11434"),
        ("ollama.num_ctx", "16384"),
        ("claude.bin", "claude"),
    ];
    let got: Vec<(&str, &str)> = config
        .entries()
        .into_iter()
        .map(|(k, s)| (k, s.map_or("(unset)", |s| s.value.as_str())))
        .collect();
    assert_eq!(got, want);
}

fn resolve_uncommented() -> Config {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, uncommented(TEMPLATE)).unwrap();
    Config::resolve(Overrides::default(), &|_| None, Some(path.as_path())).unwrap()
}
