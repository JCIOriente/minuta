use minuta::config::{env_snapshot, resolve_path, Config, Overrides, Setting, Source};
use minuta::error::{Error, Result};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

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

#[test]
fn flag_beats_env() {
    let flags = Overrides {
        whisper_lang: Some("es".into()),
        ..Overrides::default()
    };
    let cfg = Config::resolve(flags, &env_with("MINUTA_WHISPER_LANG", "en"), None).unwrap();
    assert_eq!(cfg.whisper_lang.value, "es");
    assert_eq!(cfg.whisper_lang.source, Source::Flag);
}

#[test]
fn env_beats_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "[whisper]\nlang = \"fr\"\n");
    let env = env_with("MINUTA_WHISPER_LANG", "en");
    let cfg = Config::resolve(Overrides::default(), &env, Some(file.as_path())).unwrap();
    assert_eq!(cfg.whisper_lang.value, "en");
    assert_eq!(cfg.whisper_lang.source, Source::Env);
}

#[test]
fn file_beats_default() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(
        &dir,
        "out_dir = \"/tmp/out\"\n[ffmpeg]\nbin = \"/opt/ffmpeg\"\n",
    );
    let cfg = Config::resolve(Overrides::default(), &no_env, Some(file.as_path())).unwrap();
    assert_eq!(cfg.out_dir.value, "/tmp/out");
    assert_eq!(cfg.out_dir.source, Source::File);
    assert_eq!(cfg.ffmpeg_bin.value, "/opt/ffmpeg");
    assert_eq!(cfg.ffmpeg_bin.source, Source::File);
    assert_eq!(cfg.ffprobe_bin.source, Source::Default);
}

#[test]
fn precedence_is_checked_per_key() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(
        &dir,
        "[whisper]\nbin = \"file-whisper\"\nmodel = \"file-model\"\n",
    );
    let flags = Overrides {
        whisper_model: Some("flag-model".into()),
        ..Overrides::default()
    };
    let env = env_with("MINUTA_WHISPER_BIN", "env-whisper");
    let cfg = Config::resolve(flags, &env, Some(file.as_path())).unwrap();
    assert_eq!(cfg.whisper_bin.value, "env-whisper");
    assert_eq!(cfg.whisper_bin.source, Source::Env);
    let model = cfg.whisper_model.unwrap();
    assert_eq!(model.value, "flag-model");
    assert_eq!(model.source, Source::Flag);
}

/// One row per config key: its TOML section, its exact env name, its default and accessors.
struct Case {
    key: &'static str,
    toml: &'static str,
    env: &'static str,
    default: Option<&'static str>,
    get: fn(&Config) -> Option<&Setting>,
    set: fn(&mut Overrides, String),
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            key: "out_dir",
            toml: "out_dir = \"file\"\n",
            env: "MINUTA_OUT_DIR",
            default: Some("./minuta-out"),
            get: |c| Some(&c.out_dir),
            set: |o, v| o.out_dir = Some(v),
        },
        Case {
            key: "whisper.bin",
            toml: "[whisper]\nbin = \"file\"\n",
            env: "MINUTA_WHISPER_BIN",
            default: Some("whisper-cli"),
            get: |c| Some(&c.whisper_bin),
            set: |o, v| o.whisper_bin = Some(v),
        },
        Case {
            key: "whisper.model",
            toml: "[whisper]\nmodel = \"file\"\n",
            env: "MINUTA_WHISPER_MODEL",
            default: None,
            get: |c| c.whisper_model.as_ref(),
            set: |o, v| o.whisper_model = Some(v),
        },
        Case {
            key: "whisper.lang",
            toml: "[whisper]\nlang = \"file\"\n",
            env: "MINUTA_WHISPER_LANG",
            default: Some("auto"),
            get: |c| Some(&c.whisper_lang),
            set: |o, v| o.whisper_lang = Some(v),
        },
        Case {
            key: "ffmpeg.bin",
            toml: "[ffmpeg]\nbin = \"file\"\n",
            env: "MINUTA_FFMPEG_BIN",
            default: Some("ffmpeg"),
            get: |c| Some(&c.ffmpeg_bin),
            set: |o, v| o.ffmpeg_bin = Some(v),
        },
        Case {
            key: "ffprobe.bin",
            toml: "[ffprobe]\nbin = \"file\"\n",
            env: "MINUTA_FFPROBE_BIN",
            default: Some("ffprobe"),
            get: |c| Some(&c.ffprobe_bin),
            set: |o, v| o.ffprobe_bin = Some(v),
        },
    ]
}

#[test]
fn every_key_obeys_flag_env_file_default_precedence() {
    for case in cases() {
        let dir = tempfile::tempdir().unwrap();
        let file = write_config(&dir, case.toml);
        let file = Some(file.as_path());

        let mut flags = Overrides::default();
        (case.set)(&mut flags, "flag".into());
        let cfg = Config::resolve(flags, &env_with(case.env, "env"), file).unwrap();
        assert_key((case.get)(&cfg), Some(("flag", Source::Flag)), case.key);

        let cfg = Config::resolve(Overrides::default(), &env_with(case.env, "env"), file).unwrap();
        assert_key((case.get)(&cfg), Some(("env", Source::Env)), case.key);

        let cfg = Config::resolve(Overrides::default(), &no_env, file).unwrap();
        assert_key((case.get)(&cfg), Some(("file", Source::File)), case.key);

        let cfg = Config::resolve(Overrides::default(), &no_env, None).unwrap();
        let want = case.default.map(|d| (d, Source::Default));
        assert_key((case.get)(&cfg), want, case.key);
    }
}

#[test]
fn whisper_model_unset_is_none() {
    let cfg = Config::resolve(Overrides::default(), &no_env, None).unwrap();
    assert!(cfg.whisper_model.is_none());
}

#[test]
fn empty_whisper_model_env_is_unset() {
    let cfg = Config::resolve(
        Overrides::default(),
        &env_with("MINUTA_WHISPER_MODEL", ""),
        None,
    )
    .unwrap();
    assert!(cfg.whisper_model.is_none());
}

#[test]
fn empty_whisper_model_env_falls_through_to_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "[whisper]\nmodel = \"file-model\"\n");
    let env = env_with("MINUTA_WHISPER_MODEL", "");
    let cfg = Config::resolve(Overrides::default(), &env, Some(file.as_path())).unwrap();
    let model = cfg.whisper_model.unwrap();
    assert_eq!(
        (model.value.as_str(), model.source),
        ("file-model", Source::File)
    );
}

#[test]
fn empty_flag_falls_through_to_env() {
    let flags = Overrides {
        whisper_lang: Some(String::new()),
        ..Overrides::default()
    };
    let cfg = Config::resolve(flags, &env_with("MINUTA_WHISPER_LANG", "en"), None).unwrap();
    assert_eq!(cfg.whisper_lang.value, "en");
    assert_eq!(cfg.whisper_lang.source, Source::Env);
}

#[test]
fn empty_file_value_uses_default() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "[ffmpeg]\nbin = \"\"\n");
    let cfg = Config::resolve(Overrides::default(), &no_env, Some(file.as_path())).unwrap();
    assert_eq!(cfg.ffmpeg_bin.value, "ffmpeg");
    assert_eq!(cfg.ffmpeg_bin.source, Source::Default);
}

#[test]
fn defaults_apply_without_file() {
    let cfg = Config::resolve(Overrides::default(), &no_env, None).unwrap();
    assert_eq!(cfg.out_dir.value, "./minuta-out");
    assert_eq!(cfg.whisper_lang.value, "auto");
    assert_eq!(cfg.whisper_lang.source, Source::Default);
    assert_eq!(cfg.whisper_bin.value, "whisper-cli");
    assert_eq!(cfg.ffmpeg_bin.value, "ffmpeg");
    assert_eq!(cfg.ffprobe_bin.value, "ffprobe");
}

#[test]
fn missing_default_file_uses_defaults() {
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
fn unreadable_path_is_config_error_naming_file() {
    let dir = tempfile::tempdir().unwrap();
    let env = env_with("MINUTA_CONFIG", dir.path().to_str().unwrap());
    let msg = config_err(Config::resolve(Overrides::default(), &env, None));
    assert!(msg.contains(dir.path().to_str().unwrap()), "no path: {msg}");
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

#[test]
fn file_loads_all_keys() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(
        &dir,
        "out_dir = \"o\"\n[whisper]\nbin = \"wb\"\nmodel = \"wm\"\nlang = \"es\"\n[ffmpeg]\nbin = \"fb\"\n[ffprobe]\nbin = \"pb\"\n",
    );
    let cfg = Config::resolve(Overrides::default(), &no_env, Some(file.as_path())).unwrap();
    assert_eq!(cfg.out_dir.value, "o");
    assert_eq!(cfg.whisper_bin.value, "wb");
    assert_eq!(cfg.whisper_model.unwrap().value, "wm");
    assert_eq!(cfg.whisper_lang.value, "es");
    assert_eq!(cfg.ffmpeg_bin.value, "fb");
    assert_eq!(cfg.ffprobe_bin.value, "pb");
}

#[test]
fn show_lists_every_key_with_source_or_unset() {
    let cfg = Config::resolve(Overrides::default(), &no_env, None).unwrap();
    let shown = cfg.entries();
    let keys: Vec<&str> = shown.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        keys,
        [
            "out_dir",
            "whisper.bin",
            "whisper.model",
            "whisper.lang",
            "ffmpeg.bin",
            "ffprobe.bin"
        ]
    );
    assert!(shown.iter().all(|(k, s)| match s {
        Some(s) => s.source == Source::Default,
        None => *k == "whisper.model",
    }));
}

#[test]
fn env_snapshot_reads_minuta_names() {
    let map = env_snapshot(&|k| (k == "MINUTA_OUT_DIR").then(|| OsString::from("x"))).unwrap();
    assert_eq!(map.get("MINUTA_OUT_DIR").map(String::as_str), Some("x"));
    assert!(!map.contains_key("MINUTA_CONFIG"));
}

#[test]
fn env_snapshot_rejects_non_utf8_value() {
    let bad = OsString::from_vec(vec![0xff]);
    let err = env_snapshot(&|k| (k == "MINUTA_FFPROBE_BIN").then(|| bad.clone())).unwrap_err();
    match err {
        Error::Config(msg) => assert!(msg.contains("MINUTA_FFPROBE_BIN"), "no name: {msg}"),
        other => panic!("expected Error::Config, got {other:?}"),
    }
}
