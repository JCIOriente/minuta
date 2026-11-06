//! Numeric keys: parsing, rejection of bad values, typed accessors and `llm.repairs`.

use super::*;

#[test]
fn numeric_keys_reject_garbage() {
    let rows = [
        ("llm.retries", "[llm]\nretries = \"abc\"\n"),
        ("llm.retries", "[llm]\nretries = 0\n"),
        ("llm.concurrency", "[llm]\nconcurrency = \"abc\"\n"),
        ("llm.concurrency", "[llm]\nconcurrency = 0\n"),
        ("llm.backoff_ms", "[llm]\nbackoff_ms = \"abc\"\n"),
        ("ollama.num_ctx", "[ollama]\nnum_ctx = \"abc\"\n"),
        ("ollama.num_ctx", "[ollama]\nnum_ctx = 0\n"),
        ("llm.backoff_ms", "[llm]\nbackoff_ms = 1.5\n"),
        ("llm.retries", "[llm]\nretries = true\n"),
        ("llm.repairs", "[llm]\nrepairs = -1\n"),
        ("llm.repairs", "[llm]\nrepairs = \"abc\"\n"),
        ("llm.repairs", "[llm]\nrepairs = 1.5\n"),
    ];
    for (key, toml) in rows {
        let dir = tempfile::tempdir().unwrap();
        let file = write_config(&dir, toml);
        let msg = config_err(Config::resolve(
            Overrides::default(),
            &no_env,
            Some(file.as_path()),
        ));
        assert!(
            msg.contains(key),
            "{toml:?}: message does not name {key}: {msg}"
        );
    }
}

#[test]
fn backoff_zero_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "[llm]\nbackoff_ms = 0\n");
    let cfg = Config::resolve(Overrides::default(), &no_env, Some(file.as_path())).unwrap();
    assert_eq!(cfg.backoff(), Duration::ZERO);
}

#[test]
fn typed_accessors_return_resolved_values() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(
        &dir,
        "[llm]\nconcurrency = 2\nretries = 5\nbackoff_ms = 100\n[ollama]\nnum_ctx = 8192\n",
    );
    let cfg = Config::resolve(Overrides::default(), &no_env, Some(file.as_path())).unwrap();
    assert_eq!(cfg.retries(), 5);
    assert_eq!(cfg.backoff(), Duration::from_millis(100));
    assert_eq!(cfg.num_ctx(), 8192);
    assert_eq!(cfg.concurrency(), Some(2));

    let defaults = Config::resolve(Overrides::default(), &no_env, None).unwrap();
    assert_eq!(defaults.retries(), 3);
    assert_eq!(defaults.backoff(), Duration::from_millis(2000));
    assert_eq!(defaults.num_ctx(), 16384);
    assert_eq!(defaults.concurrency(), None);
}

#[test]
fn entries_list_new_keys() {
    let cfg = Config::resolve(Overrides::default(), &no_env, None).unwrap();
    let keys: Vec<&str> = cfg.entries().iter().map(|(k, _)| *k).collect();
    assert_eq!(
        keys,
        [
            "out_dir",
            "whisper.bin",
            "whisper.model",
            "whisper.lang",
            "whisper.vad_model",
            "whisper.models_dir",
            "ffmpeg.bin",
            "ffprobe.bin",
            "llm.default",
            "llm.output_lang",
            "llm.concurrency",
            "llm.retries",
            "llm.backoff_ms",
            "llm.repairs",
            "ollama.url",
            "ollama.num_ctx",
            "claude.bin",
            "codex.bin",
        ]
    );
}

#[test]
fn unknown_llm_key_rejected() {
    for (table, toml) in [
        ("llm", "[llm]\ndefualt = \"x\"\n"),
        ("ollama", "[ollama]\nurl2 = \"x\"\n"),
        ("claude", "[claude]\nbinary = \"x\"\n"),
        ("codex", "[codex]\nbinary = \"x\"\n"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let file = write_config(&dir, toml);
        let msg = config_err(Config::resolve(
            Overrides::default(),
            &no_env,
            Some(file.as_path()),
        ));
        assert!(msg.contains("unknown field"), "[{table}]: {msg}");
        assert!(msg.contains("line 2"), "[{table}] no line number: {msg}");
    }
}

#[test]
fn numeric_env_rejects_garbage() {
    let rows = [
        ("llm.retries", "MINUTA_LLM_RETRIES"),
        ("llm.repairs", "MINUTA_LLM_REPAIRS"),
        ("llm.backoff_ms", "MINUTA_LLM_BACKOFF_MS"),
        ("llm.concurrency", "MINUTA_LLM_CONCURRENCY"),
        ("ollama.num_ctx", "MINUTA_OLLAMA_NUM_CTX"),
    ];
    for (key, env) in rows {
        let msg = config_err(Config::resolve(
            Overrides::default(),
            &env_with(env, "abc"),
            None,
        ));
        assert!(
            msg.contains(key),
            "{env}=abc: message does not name {key}: {msg}"
        );
    }
}

#[test]
fn numeric_keys_accept_toml_strings() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(
        &dir,
        "[llm]\nretries = \"5\"\n[ollama]\nnum_ctx = \"8192\"\n",
    );
    let cfg = Config::resolve(Overrides::default(), &no_env, Some(file.as_path())).unwrap();
    assert_eq!(cfg.retries(), 5);
    assert_eq!(cfg.num_ctx(), 8192);
}

#[test]
fn datetime_numeric_value_message_names_key_without_marker() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "[llm]\nretries = 1979-05-27\n");
    let msg = config_err(Config::resolve(
        Overrides::default(),
        &no_env,
        Some(file.as_path()),
    ));
    assert!(
        msg.contains("llm.retries"),
        "message does not name the key: {msg}"
    );
    assert!(
        !msg.contains("$__toml_private"),
        "leaked toml marker: {msg}"
    );
}

#[test]
fn repairs_default_is_two() {
    let cfg = Config::resolve(Overrides::default(), &no_env, None).unwrap();
    assert_eq!(cfg.repairs(), 2);
    assert_key(
        Some(&cfg.llm_repairs),
        Some(("2", Source::Default)),
        "llm.repairs",
    );
}

#[test]
fn repairs_from_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "[llm]\nrepairs = 4\n");
    let cfg = Config::resolve(Overrides::default(), &no_env, Some(file.as_path())).unwrap();
    assert_eq!(cfg.repairs(), 4);
    assert_key(
        Some(&cfg.llm_repairs),
        Some(("4", Source::File)),
        "llm.repairs",
    );
}

#[test]
fn repairs_env_overrides_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "[llm]\nrepairs = 4\n");
    let env = env_with("MINUTA_LLM_REPAIRS", "7");
    let cfg = Config::resolve(Overrides::default(), &env, Some(file.as_path())).unwrap();
    assert_eq!(cfg.repairs(), 7);
    assert_key(
        Some(&cfg.llm_repairs),
        Some(("7", Source::Env)),
        "llm.repairs",
    );
}

#[test]
fn repairs_zero_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "[llm]\nrepairs = 0\n");
    let cfg = Config::resolve(Overrides::default(), &no_env, Some(file.as_path())).unwrap();
    assert_eq!(cfg.repairs(), 0);
}

#[test]
fn repairs_rejects_non_integer() {
    let rows = [
        ("[llm]\nrepairs = -1\n", None),
        ("[llm]\nrepairs = \"abc\"\n", None),
        ("[llm]\nrepairs = 1.5\n", None),
        ("", Some("-1")),
        ("", Some("abc")),
    ];
    for (toml, env) in rows {
        let dir = tempfile::tempdir().unwrap();
        let file = write_config(&dir, toml);
        let env_value = env.unwrap_or("");
        let env_fn = env_with("MINUTA_LLM_REPAIRS", env_value);
        let read: &dyn Fn(&str) -> Option<String> = if env.is_some() { &env_fn } else { &no_env };
        let msg = config_err(Config::resolve(
            Overrides::default(),
            read,
            Some(file.as_path()),
        ));
        assert!(
            msg.contains("llm.repairs"),
            "{toml:?} / {env:?}: message does not name the key: {msg}"
        );
    }
}

#[test]
fn show_lists_repairs() {
    let cfg = Config::resolve(Overrides::default(), &no_env, None).unwrap();
    let (key, setting) = cfg
        .entries()
        .into_iter()
        .find(|(k, _)| *k == "llm.repairs")
        .expect("llm.repairs is listed");
    assert_key(setting, Some(("2", Source::Default)), key);
}
