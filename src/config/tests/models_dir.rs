//! `whisper.models_dir`: precedence and `~` expansion.

use super::*;

#[test]
fn models_dir_defaults_under_home_minuta() {
    let cfg = Config::resolve(Overrides::default(), &no_env, None).unwrap();
    assert_key(
        Some(&cfg.models_dir),
        Some(("~/.minuta/models", Source::Default)),
        "whisper.models_dir",
    );
}

#[test]
fn models_dir_env_beats_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "[whisper]\nmodels_dir = \"/from/file\"\n");
    let env = env_with("MINUTA_MODELS_DIR", "/from/env");
    let cfg = Config::resolve(Overrides::default(), &env, Some(file.as_path())).unwrap();
    assert_key(
        Some(&cfg.models_dir),
        Some(("/from/env", Source::Env)),
        "whisper.models_dir",
    );
}

#[test]
fn models_dir_file_used_when_no_env() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "[whisper]\nmodels_dir = \"/from/file\"\n");
    let cfg = Config::resolve(Overrides::default(), &no_env, Some(file.as_path())).unwrap();
    assert_key(
        Some(&cfg.models_dir),
        Some(("/from/file", Source::File)),
        "whisper.models_dir",
    );
}

#[test]
fn models_dir_empty_env_is_unset() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "[whisper]\nmodels_dir = \"/from/file\"\n");
    let env = env_with("MINUTA_MODELS_DIR", "");
    let cfg = Config::resolve(Overrides::default(), &env, Some(file.as_path())).unwrap();
    assert_key(
        Some(&cfg.models_dir),
        Some(("/from/file", Source::File)),
        "whisper.models_dir",
    );
}

#[test]
fn models_dir_empty_file_is_unset() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "[whisper]\nmodels_dir = \"\"\n");
    let cfg = Config::resolve(Overrides::default(), &no_env, Some(file.as_path())).unwrap();
    assert_key(
        Some(&cfg.models_dir),
        Some(("~/.minuta/models", Source::Default)),
        "whisper.models_dir",
    );
}

#[test]
fn models_dir_tilde_expands_against_home() {
    let home = Path::new("/home/u");
    assert_eq!(
        expand_tilde("~/.minuta/models", Some(home)).unwrap(),
        PathBuf::from("/home/u/.minuta/models"),
    );
}

#[test]
fn models_dir_bare_tilde_is_home() {
    let home = Path::new("/home/u");
    assert_eq!(
        expand_tilde("~", Some(home)).unwrap(),
        PathBuf::from("/home/u")
    );
}

#[test]
fn models_dir_absolute_path_is_not_expanded() {
    assert_eq!(
        expand_tilde("/data/models", None).unwrap(),
        PathBuf::from("/data/models"),
    );
}

#[test]
fn models_dir_tilde_without_home_is_config_error() {
    match expand_tilde("~/models", None) {
        Err(Error::Config(msg)) => assert_eq!(
            msg,
            "whisper.models_dir is \"~/models\" but the home directory is unknown; set an absolute path",
        ),
        other => panic!("expected Error::Config, got {other:?}"),
    }
}

#[test]
fn models_dir_user_form_is_config_error() {
    match expand_tilde("~alice/models", Some(Path::new("/home/u"))) {
        Err(Error::Config(msg)) => assert_eq!(
            msg,
            "whisper.models_dir is \"~alice/models\": only ~ and ~/ expand; use ~/ or an absolute path",
        ),
        other => panic!("expected Error::Config, got {other:?}"),
    }
}

#[test]
fn models_dir_trailing_slash_expands_to_home() {
    let home = Path::new("/home/u");
    assert_eq!(
        expand_tilde("~/", Some(home)).unwrap(),
        PathBuf::from("/home/u/"),
    );
}

#[test]
fn models_dir_relative_path_is_not_expanded() {
    assert_eq!(
        expand_tilde("models/here", None).unwrap(),
        PathBuf::from("models/here"),
    );
}
