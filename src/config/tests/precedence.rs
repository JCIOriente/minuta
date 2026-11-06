use super::*;

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
