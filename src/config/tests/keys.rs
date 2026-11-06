use super::*;

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
    assert!(shown.iter().all(|(k, s)| match s {
        Some(s) => s.source == Source::Default,
        None => matches!(
            *k,
            "whisper.model" | "whisper.vad_model" | "llm.output_lang" | "llm.concurrency"
        ),
    }));
}

#[test]
fn entries_map_each_key_to_its_own_file_value() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(
        &dir,
        "out_dir = \"out-v\"\n[whisper]\nbin = \"wb-v\"\nmodel = \"wm-v\"\nlang = \"wl-v\"\nvad_model = \"wv-v\"\nmodels_dir = \"md-v\"\n[ffmpeg]\nbin = \"ff-v\"\n[ffprobe]\nbin = \"fp-v\"\n[llm]\ndefault = \"ld-v\"\noutput_lang = \"lo-v\"\nconcurrency = 3\nretries = 4\nbackoff_ms = 150\nrepairs = 6\n[ollama]\nurl = \"ou-v\"\nnum_ctx = 8192\n[claude]\nbin = \"cb-v\"\n[codex]\nbin = \"cx-v\"\n",
    );
    let cfg = Config::resolve(Overrides::default(), &no_env, Some(file.as_path())).unwrap();
    let want = [
        ("out_dir", "out-v"),
        ("whisper.bin", "wb-v"),
        ("whisper.model", "wm-v"),
        ("whisper.lang", "wl-v"),
        ("whisper.vad_model", "wv-v"),
        ("whisper.models_dir", "md-v"),
        ("ffmpeg.bin", "ff-v"),
        ("ffprobe.bin", "fp-v"),
        ("llm.default", "ld-v"),
        ("llm.output_lang", "lo-v"),
        ("llm.concurrency", "3"),
        ("llm.retries", "4"),
        ("llm.backoff_ms", "150"),
        ("llm.repairs", "6"),
        ("ollama.url", "ou-v"),
        ("ollama.num_ctx", "8192"),
        ("claude.bin", "cb-v"),
        ("codex.bin", "cx-v"),
    ];
    let entries = cfg.entries();
    assert_eq!(entries.len(), want.len());
    for ((key, setting), (want_key, want_value)) in entries.iter().zip(want) {
        assert_eq!(*key, want_key);
        assert_key(*setting, Some((want_value, Source::File)), want_key);
    }
}

#[test]
fn unknown_key_in_ffmpeg_table_names_line() {
    let dir = tempfile::tempdir().unwrap();
    let file = write_config(&dir, "[ffmpeg]\nlang = \"x\"\n");
    let msg = config_err(Config::resolve(
        Overrides::default(),
        &no_env,
        Some(file.as_path()),
    ));
    assert!(
        msg.contains(&file.display().to_string()),
        "no file name: {msg}"
    );
    assert!(msg.contains("line 2"), "no line number: {msg}");
}

#[test]
fn llm_default_flag_beats_env() {
    let flags = Overrides {
        llm_default: Some("claude:sonnet".into()),
        ..Overrides::default()
    };
    let cfg = Config::resolve(flags, &env_with("MINUTA_LLM", "ollama:x"), None).unwrap();
    assert_key(
        Some(&cfg.llm_default),
        Some(("claude:sonnet", Source::Flag)),
        "llm.default",
    );
}

#[test]
fn output_lang_flag_beats_env() {
    let flags = Overrides {
        llm_output_lang: Some("Spanish".into()),
        ..Overrides::default()
    };
    let cfg = Config::resolve(flags, &env_with("MINUTA_OUTPUT_LANG", "French"), None).unwrap();
    assert_key(
        cfg.llm_output_lang.as_ref(),
        Some(("Spanish", Source::Flag)),
        "llm.output_lang",
    );
}

/// One row per new config key: its TOML text with `file_value`, its env var with `env_value`,
/// and its default (`None` when unset).
struct NewCase {
    key: &'static str,
    toml: &'static str,
    file_value: &'static str,
    env: &'static str,
    env_value: &'static str,
    default: Option<&'static str>,
    get: fn(&Config) -> Option<&Setting>,
}

fn new_cases() -> Vec<NewCase> {
    vec![
        NewCase {
            key: "whisper.vad_model",
            toml: "[whisper]\nvad_model = \"file\"\n",
            file_value: "file",
            env: "MINUTA_WHISPER_VAD_MODEL",
            env_value: "env",
            default: None,
            get: |c| c.whisper_vad_model.as_ref(),
        },
        NewCase {
            key: "llm.default",
            toml: "[llm]\ndefault = \"file\"\n",
            file_value: "file",
            env: "MINUTA_LLM",
            env_value: "env",
            default: Some("ollama:gemma4:26b"),
            get: |c| Some(&c.llm_default),
        },
        NewCase {
            key: "llm.output_lang",
            toml: "[llm]\noutput_lang = \"file\"\n",
            file_value: "file",
            env: "MINUTA_OUTPUT_LANG",
            env_value: "env",
            default: None,
            get: |c| c.llm_output_lang.as_ref(),
        },
        NewCase {
            key: "llm.concurrency",
            toml: "[llm]\nconcurrency = 2\n",
            file_value: "2",
            env: "MINUTA_LLM_CONCURRENCY",
            env_value: "4",
            default: None,
            get: |c| c.llm_concurrency.as_ref(),
        },
        NewCase {
            key: "llm.retries",
            toml: "[llm]\nretries = 5\n",
            file_value: "5",
            env: "MINUTA_LLM_RETRIES",
            env_value: "6",
            default: Some("3"),
            get: |c| Some(&c.llm_retries),
        },
        NewCase {
            key: "llm.backoff_ms",
            toml: "[llm]\nbackoff_ms = 100\n",
            file_value: "100",
            env: "MINUTA_LLM_BACKOFF_MS",
            env_value: "200",
            default: Some("2000"),
            get: |c| Some(&c.llm_backoff_ms),
        },
        NewCase {
            key: "llm.repairs",
            toml: "[llm]\nrepairs = 5\n",
            file_value: "5",
            env: "MINUTA_LLM_REPAIRS",
            env_value: "6",
            default: Some("2"),
            get: |c| Some(&c.llm_repairs),
        },
        NewCase {
            key: "ollama.url",
            toml: "[ollama]\nurl = \"file\"\n",
            file_value: "file",
            env: "MINUTA_OLLAMA_URL",
            env_value: "env",
            default: Some("http://127.0.0.1:11434"),
            get: |c| Some(&c.ollama_url),
        },
        NewCase {
            key: "ollama.num_ctx",
            toml: "[ollama]\nnum_ctx = 8192\n",
            file_value: "8192",
            env: "MINUTA_OLLAMA_NUM_CTX",
            env_value: "4096",
            default: Some("16384"),
            get: |c| Some(&c.ollama_num_ctx),
        },
        NewCase {
            key: "claude.bin",
            toml: "[claude]\nbin = \"file\"\n",
            file_value: "file",
            env: "MINUTA_CLAUDE_BIN",
            env_value: "env",
            default: Some("claude"),
            get: |c| Some(&c.claude_bin),
        },
        NewCase {
            key: "codex.bin",
            toml: "[codex]\nbin = \"file\"\n",
            file_value: "file",
            env: "MINUTA_CODEX_BIN",
            env_value: "env",
            default: Some("codex"),
            get: |c| Some(&c.codex_bin),
        },
    ]
}

#[test]
fn each_new_key_env_beats_file() {
    for case in new_cases() {
        let dir = tempfile::tempdir().unwrap();
        let file = write_config(&dir, case.toml);
        let env = env_with(case.env, case.env_value);
        let cfg = Config::resolve(Overrides::default(), &env, Some(file.as_path())).unwrap();
        assert_key(
            (case.get)(&cfg),
            Some((case.env_value, Source::Env)),
            case.key,
        );
    }
}

#[test]
fn each_new_key_file_beats_default() {
    for case in new_cases() {
        let dir = tempfile::tempdir().unwrap();
        let file = write_config(&dir, case.toml);
        let cfg = Config::resolve(Overrides::default(), &no_env, Some(file.as_path())).unwrap();
        assert_key(
            (case.get)(&cfg),
            Some((case.file_value, Source::File)),
            case.key,
        );
    }
}

#[test]
fn each_new_key_default() {
    for case in new_cases() {
        let cfg = Config::resolve(Overrides::default(), &no_env, None).unwrap();
        let want = case.default.map(|d| (d, Source::Default));
        assert_key((case.get)(&cfg), want, case.key);
    }
}
