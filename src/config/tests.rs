//! Tests for `Config::resolve` and its helpers, split by behavior.

use super::*;

mod endpoints;
mod env;
mod file;
mod keys;
mod models_dir;
mod numeric;
mod precedence;

fn no_env(_: &str) -> Option<String> {
    None
}

/// An env closure that exposes exactly one variable.
fn env_with(name: &str, value: &str) -> impl Fn(&str) -> Option<String> {
    let (name, value) = (name.to_string(), value.to_string());
    move |k: &str| (k == name).then(|| value.clone())
}

fn write_config(dir: &tempfile::TempDir, text: &str) -> PathBuf {
    let path = dir.path().join("config.toml");
    std::fs::write(&path, text).unwrap();
    path
}

fn config_err(result: Result<Config>) -> String {
    match result {
        Err(Error::Config(msg)) => msg,
        Err(other) => panic!("expected Error::Config, got {other:?}"),
        Ok(cfg) => panic!("expected an error, got {cfg:?}"),
    }
}

fn assert_key(actual: Option<&Setting>, want: Option<(&str, Source)>, key: &str) {
    let got = actual.map(|s| (s.value.as_str(), s.source));
    assert_eq!(got, want, "key {key}");
}
