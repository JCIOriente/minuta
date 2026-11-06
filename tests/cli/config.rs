//! `minuta config path` and `minuta config show`.

use std::fs;

use assert_cmd::assert::OutputAssertExt;
use predicates::str::contains;

use crate::common::*;
use crate::fake_tools::*;

#[test]
fn config_path_prints_path_even_when_malformed() {
    let env = FakeEnv::new();
    fs::write(env.path("config.toml"), "out_dir = \n").unwrap();
    let out = env.minuta(&["config", "path"]);
    out.clone().assert().success();
    assert_eq!(
        stdout(&out),
        format!("{}\n", env.path("config.toml").display())
    );
    env.minuta(&["config", "show"])
        .assert()
        .code(1)
        .stderr(contains("line 1"));
}

#[test]
fn config_show_models_dir_from_env() {
    let env = FakeEnv::new();
    let out = env.run_cmd({
        let mut cmd = env.command(&["config", "show"]);
        cmd.env("MINUTA_MODELS_DIR", "/data/models");
        cmd
    });
    out.clone().assert().success();
    assert!(
        stdout(&out).contains("whisper.models_dir = /data/models (env)\n"),
        "stdout was {}",
        stdout(&out)
    );
}

#[test]
fn config_show_lists_values_and_sources() {
    let env = FakeEnv::new();
    fs::write(env.path("config.toml"), "[whisper]\nlang = \"es\"\n").unwrap();
    let out = env.run_cmd({
        let mut cmd = env.command(&["config", "show"]);
        cmd.env_remove("MINUTA_WHISPER_MODEL")
            .env("MINUTA_FFMPEG_BIN", "/opt/ffmpeg")
            .env("MINUTA_WHISPER_VAD_MODEL", "silero-v5.1.2")
            .env("MINUTA_CODEX_BIN", "/opt/codex");
        cmd
    });
    out.clone().assert().success();
    assert_eq!(
        stdout(&out),
        "out_dir = ./minuta-out (default)\n\
         whisper.bin = whisper-cli (default)\n\
         whisper.model = (unset)\n\
         whisper.lang = es (file)\n\
         whisper.vad_model = silero-v5.1.2 (env)\n\
         whisper.models_dir = ~/.minuta/models (default)\n\
         ffmpeg.bin = /opt/ffmpeg (env)\n\
         ffprobe.bin = ffprobe (default)\n\
         llm.default = ollama:gemma4:26b (default)\n\
         llm.output_lang = (unset)\n\
         llm.concurrency = (unset)\n\
         llm.retries = 3 (default)\n\
         llm.backoff_ms = 2000 (default)\n\
         llm.repairs = 2 (default)\n\
         ollama.url = http://127.0.0.1:11434 (default)\n\
         ollama.num_ctx = 16384 (default)\n\
         claude.bin = claude (default)\n\
         codex.bin = /opt/codex (env)\n"
    );
}
