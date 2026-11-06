use super::*;

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

#[test]
fn env_snapshot_covers_every_minuta_name() {
    let dir = tempfile::tempdir().unwrap();
    let config = write_config(&dir, "");
    let values: Vec<(&str, String)> = vec![
        ("MINUTA_CONFIG", config.display().to_string()),
        ("MINUTA_OUT_DIR", "env-out".into()),
        ("MINUTA_WHISPER_BIN", "env-wb".into()),
        ("MINUTA_WHISPER_MODEL", "env-wm".into()),
        ("MINUTA_WHISPER_LANG", "env-wl".into()),
        ("MINUTA_WHISPER_VAD_MODEL", "env-wv".into()),
        ("MINUTA_MODELS_DIR", "env-md".into()),
        ("MINUTA_FFMPEG_BIN", "env-ff".into()),
        ("MINUTA_FFPROBE_BIN", "env-fp".into()),
        ("MINUTA_LLM", "env-ld".into()),
        ("MINUTA_OUTPUT_LANG", "env-lo".into()),
        ("MINUTA_LLM_CONCURRENCY", "2".into()),
        ("MINUTA_LLM_RETRIES", "5".into()),
        ("MINUTA_LLM_BACKOFF_MS", "0".into()),
        ("MINUTA_LLM_REPAIRS", "1".into()),
        ("MINUTA_OLLAMA_URL", "env-ou".into()),
        ("MINUTA_OLLAMA_NUM_CTX", "4096".into()),
        ("MINUTA_CLAUDE_BIN", "env-cb".into()),
        ("MINUTA_CODEX_BIN", "env-cx".into()),
    ];
    let read = |k: &str| {
        values
            .iter()
            .find(|(name, _)| *name == k)
            .map(|(_, v)| OsString::from(v))
    };
    let snapshot = env_snapshot(&read).unwrap();
    assert_eq!(snapshot.len(), values.len(), "snapshot: {snapshot:?}");
    for (name, value) in &values {
        assert_eq!(snapshot.get(*name), Some(value), "name {name}");
    }

    let cfg = Config::resolve(Overrides::default(), &|k| snapshot.get(k).cloned(), None).unwrap();
    assert_key(
        cfg.entries()[0].1,
        Some(("env-out", Source::Env)),
        "out_dir",
    );
    assert_key(
        cfg.entries()[1].1,
        Some(("env-wb", Source::Env)),
        "whisper.bin",
    );
    assert_key(
        cfg.entries()[2].1,
        Some(("env-wm", Source::Env)),
        "whisper.model",
    );
    assert_key(
        cfg.entries()[3].1,
        Some(("env-wl", Source::Env)),
        "whisper.lang",
    );
    assert_key(
        cfg.entries()[4].1,
        Some(("env-wv", Source::Env)),
        "whisper.vad_model",
    );
    assert_key(
        cfg.entries()[5].1,
        Some(("env-md", Source::Env)),
        "whisper.models_dir",
    );
    assert_key(
        cfg.entries()[6].1,
        Some(("env-ff", Source::Env)),
        "ffmpeg.bin",
    );
    assert_key(
        cfg.entries()[7].1,
        Some(("env-fp", Source::Env)),
        "ffprobe.bin",
    );
    assert_eq!(cfg.path, Some(config));
}
