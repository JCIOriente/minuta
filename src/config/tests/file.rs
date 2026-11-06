use super::*;

#[test]
fn missing_file_uses_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("absent.toml");
    let cfg = Config::resolve(Overrides::default(), &no_env, Some(file.as_path())).unwrap();
    assert_eq!(cfg.out_dir.value, "./minuta-out");
    assert_eq!(cfg.path, Some(file));
}

#[test]
fn missing_explicit_file_is_error() {
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("absent.toml");
    let env = env_with("MINUTA_CONFIG", absent.to_str().unwrap());
    let msg = config_err(Config::resolve(Overrides::default(), &env, None));
    assert_eq!(msg, format!("{}: not found", absent.display()));
}

#[test]
fn malformed_file_names_line() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "out_dir = \"ok\"\n\n[whisper\nbin = \"x\"\n");
    let msg = config_err(Config::resolve(
        Overrides::default(),
        &no_env,
        Some(file.as_path()),
    ));
    assert!(
        msg.contains(&file.display().to_string()),
        "no file name: {msg}"
    );
    assert!(msg.contains("line 3"), "no line number: {msg}");
}

#[test]
fn unknown_key_names_file_and_line() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "[whisper]\nbin = \"x\"\nmodle = \"y\"\n");
    let msg = config_err(Config::resolve(
        Overrides::default(),
        &no_env,
        Some(file.as_path()),
    ));
    assert!(
        msg.contains(&file.display().to_string()),
        "no file name: {msg}"
    );
    assert!(msg.contains("line 3"), "no line number: {msg}");
}

#[test]
fn minuta_config_env_overrides_path() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "out_dir = \"from-env-path\"\n");
    let env = env_with("MINUTA_CONFIG", file.to_str().unwrap());
    let default = Path::new("/nonexistent/default.toml");
    let cfg = Config::resolve(Overrides::default(), &env, Some(default)).unwrap();
    assert_eq!(cfg.path, Some(file));
    assert_eq!(cfg.out_dir.value, "from-env-path");
    assert_eq!(cfg.out_dir.source, Source::File);
}

#[test]
fn resolve_path_prefers_env_over_default() {
    let env = env_with("MINUTA_CONFIG", "/from/env.toml");
    let default = Path::new("/default.toml");
    assert_eq!(
        resolve_path(&env, Some(default)),
        Some(PathBuf::from("/from/env.toml"))
    );
}

#[test]
fn resolve_path_uses_default_without_env() {
    let default = Path::new("/default.toml");
    assert_eq!(
        resolve_path(&no_env, Some(default)),
        Some(PathBuf::from("/default.toml"))
    );
    assert_eq!(resolve_path(&no_env, None), None);
}

#[test]
fn resolve_path_ignores_empty_env() {
    let default = Path::new("/default.toml");
    let env = env_with("MINUTA_CONFIG", "");
    assert_eq!(
        resolve_path(&env, Some(default)),
        Some(PathBuf::from("/default.toml"))
    );
}

#[test]
fn malformed_file_still_resolves_path() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "[whisper\n");
    let env = env_with("MINUTA_CONFIG", file.to_str().unwrap());
    assert_eq!(resolve_path(&env, None), Some(file));
    assert!(Config::resolve(Overrides::default(), &env, None).is_err());
}

/// The message, path, source kind and source message of an `Error::ConfigIo`, or a panic
/// naming what came back instead.
fn config_io_err(result: Result<Config>) -> (String, PathBuf, std::io::ErrorKind, String) {
    let err = match result {
        Err(err @ Error::ConfigIo { .. }) => err,
        Err(other) => panic!("expected Error::ConfigIo, got {other:?}"),
        Ok(cfg) => panic!("expected an error, got {cfg:?}"),
    };
    let message = err.to_string();
    let source = std::error::Error::source(&err)
        .and_then(|s| s.downcast_ref::<std::io::Error>())
        .map(|s| (s.kind(), s.to_string()))
        .expect("ConfigIo keeps its io::Error as the source");
    let Error::ConfigIo { path, .. } = err else {
        unreachable!("matched ConfigIo above")
    };
    (message, path, source.0, source.1)
}

/// A config file with mode 000, so reading it fails with `PermissionDenied` (unless root).
fn unreadable_config(dir: &tempfile::TempDir) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = write_config(dir, "out_dir = \"x\"\n");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    path
}

#[test]
fn config_unreadable_file_keeps_io_kind_for_directory() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_with("MINUTA_CONFIG", dir.path().to_str().unwrap());
    let (message, path, kind, source) =
        config_io_err(Config::resolve(Overrides::default(), &env, None));
    assert_eq!(kind, std::io::ErrorKind::IsADirectory);
    assert_eq!(source, "Is a directory (os error 21)");
    assert_eq!(path, dir.path());
    assert_eq!(
        message,
        format!(
            "config error: {}: Is a directory (os error 21)",
            dir.path().display()
        )
    );
}

#[test]
fn config_unreadable_file_keeps_io_kind_for_explicit_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = unreadable_config(&dir);
    let env = env_with("MINUTA_CONFIG", file.to_str().unwrap());
    let (message, path, kind, source) =
        config_io_err(Config::resolve(Overrides::default(), &env, None));
    assert_eq!(kind, std::io::ErrorKind::PermissionDenied);
    assert_eq!(source, "Permission denied (os error 13)");
    assert_eq!(path, file);
    assert_eq!(
        message,
        format!(
            "config error: {}: Permission denied (os error 13)",
            file.display()
        )
    );
}

#[test]
fn config_unreadable_file_keeps_io_kind_for_default_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = unreadable_config(&dir);
    let (message, path, kind, source) = config_io_err(Config::resolve(
        Overrides::default(),
        &no_env,
        Some(file.as_path()),
    ));
    assert_eq!(kind, std::io::ErrorKind::PermissionDenied);
    assert_eq!(source, "Permission denied (os error 13)");
    assert_eq!(path, file);
    assert_eq!(
        message,
        format!(
            "config error: {}: Permission denied (os error 13)",
            file.display()
        )
    );
}

#[test]
fn config_parse_error_message() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "out_dir = \"ok\"\n\n[whisper\nbin = \"x\"\n");
    let env = env_with("MINUTA_CONFIG", file.to_str().unwrap());
    let err = Config::resolve(Overrides::default(), &env, None).unwrap_err();
    assert!(matches!(err, Error::Config(_)), "{err:?}");
    assert_eq!(
        err.to_string(),
        format!(
            "config error: {}: invalid TOML at line 3: unclosed table, expected `]`",
            file.display()
        )
    );
}

#[test]
fn default_path_is_under_home_minuta() {
    assert_eq!(
        default_path_in(Path::new("/home/u")),
        PathBuf::from("/home/u/.minuta/config.toml"),
    );
}

#[test]
fn default_path_ignores_old_config_location() {
    let home = tempfile::tempdir().unwrap();
    let old = home.path().join(".config/minuta");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("config.toml"), "out_dir = \"from-old-location\"\n").unwrap();
    let default = default_path_in(home.path());
    let cfg = Config::resolve(Overrides::default(), &no_env, Some(default.as_path())).unwrap();
    assert_eq!(cfg.path, Some(home.path().join(".minuta/config.toml")));
    assert_eq!(cfg.out_dir.value, "./minuta-out");
    assert_eq!(cfg.out_dir.source, Source::Default);
}

#[test]
fn resolve_path_env_beats_default() {
    let home = tempfile::tempdir().unwrap();
    let minuta = home.path().join(".minuta");
    std::fs::create_dir_all(&minuta).unwrap();
    std::fs::write(minuta.join("config.toml"), "out_dir = \"from-default\"\n").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "out_dir = \"from-env-path\"\n");
    let env = env_with("MINUTA_CONFIG", file.to_str().unwrap());
    let default = default_path_in(home.path());
    assert_eq!(
        resolve_path(&env, Some(default.as_path())),
        Some(file.clone())
    );
    let cfg = Config::resolve(Overrides::default(), &env, Some(default.as_path())).unwrap();
    assert_eq!(cfg.path, Some(file));
    assert_eq!(cfg.out_dir.value, "from-env-path");
    assert_eq!(cfg.out_dir.source, Source::File);
}

#[test]
fn missing_default_file_yields_defaults() {
    let home = tempfile::tempdir().unwrap();
    let default = default_path_in(home.path());
    let cfg = Config::resolve(Overrides::default(), &no_env, Some(default.as_path())).unwrap();
    assert_eq!(cfg.path, Some(home.path().join(".minuta/config.toml")));
    assert_eq!(cfg.out_dir.value, "./minuta-out");
    assert_eq!(cfg.out_dir.source, Source::Default);
}
