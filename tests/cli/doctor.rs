//! `minuta doctor`: tool versions, the exit code and the LLM line.

use std::fs;
use std::sync::Arc;

use assert_cmd::assert::OutputAssertExt;

use crate::common::fake_ollama::{FakeOllama, Reply};
use crate::common::*;
use crate::fake_tools::*;

#[test]
fn doctor_exit_code_when_missing() {
    let env = FakeEnv::new();
    let out = env.minuta(&["doctor"]);
    out.clone().assert().success();
    let report = stdout(&out);
    assert!(report.contains("ffmpeg version 7.1-fake"), "{report}");
    assert!(report.contains("ffprobe version 7.1-fake"), "{report}");
    assert!(report.contains("whisper-cli 1.7.2-fake"), "{report}");
    assert!(
        report.contains(&env.path("ggml-test.bin").display().to_string()),
        "{report}"
    );

    fs::remove_file(env.bin().join("ffprobe")).unwrap();
    let out = env.minuta(&["doctor"]);
    out.clone().assert().code(1);
    assert!(stdout(&out).contains("ffmpeg version 7.1-fake"));
    assert!(stdout(&out).contains("missing"), "{}", stdout(&out));
    assert!(stderr(&out).contains("ffprobe"), "{}", stderr(&out));
}

#[test]
fn doctor_reports_llm_without_affecting_exit_code() {
    let env = FakeEnv::new();
    let fake = ollama("unused");
    let out = env.minuta_llm(&["doctor"], &fake);
    out.clone().assert().success();
    assert!(
        stdout(&out).contains("llm          ollama:gemma4:26b (ok)"),
        "{}",
        stdout(&out)
    );

    let out = output(&mut {
        let mut cmd = env.llm_command(&["doctor"], &fake);
        cmd.env("MINUTA_LLM", "ollama:missing:1b");
        cmd
    });
    out.clone().assert().success();
    assert!(
        stdout(&out)
            .contains("llm          ollama:missing:1b (model missing: ollama pull missing:1b)"),
        "{}",
        stdout(&out)
    );

    let out = output(&mut {
        let mut cmd = env.command(&["doctor"]);
        cmd.env("MINUTA_OLLAMA_URL", NO_OLLAMA);
        cmd
    });
    out.clone().assert().success();
    assert!(
        stdout(&out).contains("llm          ollama:gemma4:26b (not reachable: "),
        "{}",
        stdout(&out)
    );
}

#[test]
fn doctor_reports_codex() {
    let env = FakeEnv::new();
    let fake = ollama("unused");
    let out = output(&mut {
        let mut cmd = env.llm_command(&["doctor"], &fake);
        cmd.env("MINUTA_LLM", "codex");
        cmd
    });
    out.clone().assert().success();
    assert!(
        stdout(&out).contains("llm          codex (ok)"),
        "{}",
        stdout(&out)
    );

    let out = output(&mut {
        let mut cmd = env.llm_command(&["doctor"], &fake);
        cmd.env("MINUTA_LLM", "codex:gpt-x")
            .env("MINUTA_CODEX_BIN", "minuta-no-such-codex");
        cmd
    });
    out.clone().assert().success();
    assert!(
        stdout(&out).contains(
            "llm          codex:gpt-x (not reachable: install Codex: npm install -g @openai/codex)"
        ),
        "{}",
        stdout(&out)
    );
}

#[test]
fn doctor_reports_ollama_show_failure_as_not_reachable() {
    let env = FakeEnv::new();
    let fake =
        FakeOllama::start_with_show(&["gemma4:26b"], None, Arc::new(|_| Reply::chat("unused")));
    let out = env.minuta_llm(&["doctor"], &fake);
    out.clone().assert().success();
    let report = stdout(&out);
    assert!(
        report.contains("llm          ollama:gemma4:26b (not reachable: "),
        "{report}"
    );
    assert!(!report.contains("model missing"), "{report}");
}
