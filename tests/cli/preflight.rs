use std::fs;
use std::sync::Arc;

use assert_cmd::assert::OutputAssertExt;

use crate::common::fake_ollama::{FakeOllama, Reply};
use crate::common::*;
use crate::fake_tools::*;

#[test]
fn preflight_before_first_step() {
    let env = FakeEnv::new();
    fs::remove_file(env.bin().join("whisper-cli")).unwrap();
    let out = env.minuta(&["run", "clase-4.mp4"]);
    out.clone().assert().code(1).stdout("");
    let err = stderr(&out);
    assert!(err.contains("whisper-cli"), "stderr was {err}");
    // Homebrew installs the tool on macOS and Linux; any other host is sent to the guide.
    let hint = if cfg!(any(target_os = "macos", target_os = "linux")) {
        "brew install whisper.cpp"
    } else {
        "docs/install.md"
    };
    assert!(err.contains(hint), "stderr was {err}");
    assert!(!err.contains("[1/5]"), "no step may start: {err}");
    assert!(env.calls().is_empty());
    assert!(!env.path("minuta-out").exists());
}

#[test]
fn preflight_lists_all_missing_at_once() {
    let env = FakeEnv::new();
    for tool in ["ffmpeg", "ffprobe", "whisper-cli"] {
        fs::remove_file(env.bin().join(tool)).unwrap();
    }
    let out = env.run_cmd({
        let mut cmd = env.command(&["run", "clase-4.mp4"]);
        cmd.env_remove("MINUTA_WHISPER_MODEL");
        cmd
    });
    out.clone().assert().code(1).stdout("");
    let err = stderr(&out);
    for item in ["ffmpeg", "ffprobe", "whisper-cli", "model"] {
        assert!(err.contains(&format!("  {item}: ")), "stderr was {err}");
    }
    assert!(err.contains("minuta models pull base"), "stderr was {err}");
}

#[test]
fn catalog_model_not_pulled_hints_pull() {
    let env = FakeEnv::new();
    let out = env.minuta(&["run", "clase-4.mp4", "--model", "small"]);
    out.clone().assert().code(1);
    assert!(
        stderr(&out).contains("model: run `minuta models pull small`"),
        "stderr was {}",
        stderr(&out)
    );
}

#[test]
fn unknown_template_exits_2_before_job_folder() {
    let env = FakeEnv::new();
    let fake = ollama("unused");
    let out = env.minuta_llm(&["run", "clase-4.mp4", "-t", "nope"], &fake);
    out.clone().assert().code(2).stdout("");
    let err = stderr(&out);
    assert!(err.contains("unknown template 'nope'"), "{err}");
    assert!(err.contains("summary"), "lists the available names: {err}");
    assert!(!env.path("minuta-out").exists());
    assert!(env.calls().is_empty());
    assert!(fake.requests().is_empty());
}

#[test]
fn resolution_problems_exit_2_before_job_folder() {
    let env = FakeEnv::new();
    env.template("templates", "x", "---\ninput: y\n---\nX.");
    env.template("templates", "y", "---\ninput: x\n---\nY.");
    env.template("templates", "bad", "---\nllm: gpt:4\n---\nBad.");
    let fake = ollama("unused");
    let cases: [(&[&str], &str); 5] = [
        (&["-t", "x"], "template cycle"),
        (
            &["-t", "summary", "--llm", "gpt:4"],
            "unknown endpoint \"gpt\"",
        ),
        (&["-t", "summary", "--output-lang", "xx"], "'xx'"),
        (&["--output-lang", "xx"], "'xx'"),
        (&["-t", "bad"], "unknown endpoint \"gpt\""),
    ];
    for (extra, expected) in cases {
        let mut args = vec!["run", "clase-4.mp4"];
        args.extend(extra);
        let out = env.minuta_llm(&args, &fake);
        assert_eq!(out.status.code(), Some(2), "{extra:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains(expected),
            "{extra:?}: {}",
            stderr(&out)
        );
    }
    assert!(!env.path("minuta-out").exists());
    assert!(fake.requests().is_empty());
}

#[test]
fn preflight_reports_backend_and_tools_together() {
    let env = FakeEnv::new();
    fs::remove_file(env.bin().join("whisper-cli")).unwrap();
    let out = env.run_cmd({
        let mut cmd = env.command(&["run", "clase-4.mp4", "-t", "summary"]);
        cmd.env("MINUTA_OLLAMA_URL", NO_OLLAMA);
        cmd
    });
    out.clone().assert().code(1).stdout("");
    let err = stderr(&out);
    assert!(err.contains("  whisper-cli: "), "{err}");
    assert!(
        err.contains("  ollama: start Ollama (`ollama serve`)"),
        "{err}"
    );
    assert!(!env.path("minuta-out").exists());
}

#[test]
fn preflight_reports_missing_ollama_model_and_claude() {
    let env = FakeEnv::new();
    env.template("templates", "acta", "---\nllm: claude\n---\nMinutes.");
    let fake = FakeOllama::start(
        &["other:7b"],
        serde_json::json!({}),
        Arc::new(|_| Reply::chat("unused")),
    );
    let out = env.run_cmd({
        let mut cmd = env.llm_command(
            &["run", "clase-4.mp4", "-t", "summary", "-t", "acta"],
            &fake,
        );
        cmd.env(
            "MINUTA_CLAUDE_BIN",
            env.path("no-claude").display().to_string(),
        );
        cmd
    });
    out.clone().assert().code(1);
    let err = stderr(&out);
    assert!(
        err.contains("  ollama:gemma4:26b: ollama pull gemma4:26b"),
        "{err}"
    );
    assert!(err.contains("  claude: "), "{err}");
    assert!(!env.path("minuta-out").exists());
}

#[test]
fn missing_codex_is_reported_with_hint() {
    let env = FakeEnv::new();
    let fake = ollama("unused");
    let out = env.run_cmd({
        let mut cmd = env.llm_command(
            &["run", "clase-4.mp4", "-t", "summary", "--llm", "codex"],
            &fake,
        );
        cmd.env("MINUTA_CODEX_BIN", "minuta-no-such-codex");
        cmd
    });
    out.clone().assert().code(1);
    let err = stderr(&out);
    assert!(
        err.contains("  codex: install Codex: npm install -g @openai/codex"),
        "{err}"
    );
    assert!(!env.path("minuta-out").exists());
}
