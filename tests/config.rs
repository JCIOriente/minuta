use minuta::config::{Config, Overrides, Source};
use minuta::error::Error;
use std::path::PathBuf;

fn no_env(_: &str) -> Option<String> {
    None
}

fn write_config(dir: &tempfile::TempDir, text: &str) -> String {
    let path = dir.path().join("config.toml");
    std::fs::write(&path, text).unwrap();
    path.to_str().unwrap().to_string()
}

#[test]
fn flag_beats_env() {
    let flags = Overrides {
        whisper_lang: Some("es".into()),
        ..Overrides::default()
    };
    let env = |k: &str| (k == "MINUTA_WHISPER_LANG").then(|| "en".to_string());
    let cfg = Config::resolve(flags, &env, None).unwrap();
    assert_eq!(cfg.whisper_lang.value, "es");
    assert_eq!(cfg.whisper_lang.source, Source::Flag);
}

#[test]
fn env_beats_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "[whisper]\nlang = \"fr\"\n");
    let env = |k: &str| (k == "MINUTA_WHISPER_LANG").then(|| "en".to_string());
    let cfg = Config::resolve(Overrides::default(), &env, Some(&file)).unwrap();
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
    let cfg = Config::resolve(Overrides::default(), &no_env, Some(&file)).unwrap();
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
    let env = |k: &str| (k == "MINUTA_WHISPER_BIN").then(|| "env-whisper".to_string());
    let cfg = Config::resolve(flags, &env, Some(&file)).unwrap();
    assert_eq!(cfg.whisper_bin.value, "env-whisper");
    assert_eq!(cfg.whisper_model.value, "flag-model");
    assert_eq!(cfg.whisper_model.source, Source::Flag);
}

#[test]
fn defaults_apply_without_file() {
    let cfg = Config::resolve(Overrides::default(), &no_env, None).unwrap();
    assert_eq!(cfg.out_dir.value, "./minuta-out");
    assert_eq!(cfg.whisper_lang.value, "auto");
    assert_eq!(cfg.whisper_lang.source, Source::Default);
}

#[test]
fn missing_file_uses_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("absent.toml");
    let file = file.to_str().unwrap();
    let cfg = Config::resolve(Overrides::default(), &no_env, Some(file)).unwrap();
    assert_eq!(cfg.out_dir.value, "./minuta-out");
    assert_eq!(cfg.path, Some(PathBuf::from(file)));
}

#[test]
fn malformed_file_names_line() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "out_dir = \"ok\"\n\n[whisper\nbin = \"x\"\n");
    let err = Config::resolve(Overrides::default(), &no_env, Some(&file)).unwrap_err();
    match err {
        Error::Config(msg) => {
            assert!(msg.contains(&file), "message lacks file name: {msg}");
            assert!(msg.contains("line 3"), "message lacks line number: {msg}");
        }
        other => panic!("expected Error::Config, got {other:?}"),
    }
}

#[test]
fn minuta_config_env_overrides_path() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "out_dir = \"from-env-path\"\n");
    let env = |k: &str| (k == "MINUTA_CONFIG").then(|| file.clone());
    let cfg = Config::resolve(
        Overrides::default(),
        &env,
        Some("/nonexistent/default.toml"),
    )
    .unwrap();
    assert_eq!(cfg.path, Some(PathBuf::from(&file)));
    assert_eq!(cfg.out_dir.value, "from-env-path");
    assert_eq!(cfg.out_dir.source, Source::File);
}

#[test]
fn file_loads_all_keys() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(
        &dir,
        "out_dir = \"o\"\n[whisper]\nbin = \"wb\"\nmodel = \"wm\"\nlang = \"es\"\n[ffmpeg]\nbin = \"fb\"\n[ffprobe]\nbin = \"pb\"\n",
    );
    let cfg = Config::resolve(Overrides::default(), &no_env, Some(&file)).unwrap();
    assert_eq!(cfg.out_dir.value, "o");
    assert_eq!(cfg.whisper_bin.value, "wb");
    assert_eq!(cfg.whisper_model.value, "wm");
    assert_eq!(cfg.whisper_lang.value, "es");
    assert_eq!(cfg.ffmpeg_bin.value, "fb");
    assert_eq!(cfg.ffprobe_bin.value, "pb");
}

#[test]
fn show_lists_every_key_with_source() {
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
    assert!(shown.iter().all(|(_, s)| s.source == Source::Default));
}
