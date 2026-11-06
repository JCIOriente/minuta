//! Runs with templates against the fake Ollama, the fake `claude` and the fake `codex`.

use std::fs;
use std::sync::Arc;

use assert_cmd::assert::OutputAssertExt;

use crate::common::fake_ollama::Reply;
use crate::common::*;
use crate::fake_tools::*;

#[test]
fn run_accepts_repeated_templates() {
    let env = FakeEnv::new();
    env.template("templates", "a", "Write A.");
    env.template("templates", "b", "Write B.");
    let fake = ollama("generated text");
    let out = env.minuta_llm(&["run", "clase-4.mp4", "-t", "a", "--template", "b"], &fake);
    out.clone().assert().success();
    let job = env.job_dir("clase-4");
    assert_eq!(stdout(&out), format!("{}\n", job.display()));
    for name in ["a", "b"] {
        assert_eq!(
            fs::read_to_string(job.join(format!("{name}.md"))).unwrap(),
            "generated text\n",
            "{name}"
        );
        assert_eq!(template_state(&job, name)["status"], "done", "{name}");
    }
    assert_eq!(fake.chat_count(), 2);
}

#[test]
fn transcribe_rejects_template_flag() {
    let env = FakeEnv::new();
    env.minuta(&["transcribe", "clase-4.mp4", "-t", "summary"])
        .assert()
        .code(2);
    env.minuta(&["transcribe", "clase-4.mp4", "--template", "summary"])
        .assert()
        .code(2);
    env.minuta(&["transcribe", "clase-4.mp4", "--llm", "claude"])
        .assert()
        .code(2);
    env.minuta(&["transcribe", "clase-4.mp4", "--output-lang", "en"])
        .assert()
        .code(2);
    assert!(!env.path("minuta-out").exists());
}

#[test]
fn run_without_templates_contacts_no_backend() {
    let env = FakeEnv::new();
    let fake = ollama("unused");
    env.minuta_llm(&["run", "clase-4.mp4"], &fake)
        .assert()
        .success();
    assert!(fake.requests().is_empty());
    assert!(!state_dir(&env.job_dir("clase-4"))
        .join("llm-cache")
        .exists());
}

#[test]
fn second_template_run_is_cached() {
    let env = FakeEnv::new();
    let fake = ollama("summary text");
    env.minuta_llm(&["run", "clase-4.mp4", "-t", "summary"], &fake)
        .assert()
        .success();
    let out = env.minuta_llm(&["run", "clase-4.mp4", "-t", "summary"], &fake);
    out.clone().assert().success();
    assert_eq!(fake.chat_count(), 1);
    let err = stderr(&out);
    assert_eq!(cached_steps(&err).len(), 6, "stderr was {err}");
    assert!(err.contains("[6/6] summary     cached"), "stderr was {err}");
    assert!(err.contains("(0 run, 6 cached)"), "stderr was {err}");
}

#[test]
fn progress_template_line_format() {
    let env = FakeEnv::new();
    let fake = ollama("summary text");
    let out = env.minuta_llm(&["run", "clase-4.mp4", "-t", "summary"], &fake);
    out.clone().assert().success();
    let err = stderr(&out);
    let line = err
        .lines()
        .find(|l| l.starts_with("[6/6] summary"))
        .unwrap_or_else(|| panic!("no template line in {err}"));
    let rest = line
        .strip_prefix("[6/6] summary     ollama:gemma4:26b  1 call  ")
        .unwrap_or_else(|| panic!("line was {line:?}"));
    assert!(rest.ends_with('s'), "time: {rest:?}");
}

#[test]
fn progress_total_includes_templates() {
    let env = FakeEnv::new();
    env.template("templates", "a", "Write A.");
    env.template("templates", "b", "---\ninput: a\n---\nWrite B.");
    let fake = ollama("text");
    let out = env.minuta_llm(&["run", "clase-4.mp4", "-t", "b"], &fake);
    out.clone().assert().success();
    let err = stderr(&out);
    for line in ["[1/7] probe", "[5/7] outputs", "[6/7] a    ", "[7/7] b    "] {
        assert!(err.contains(line), "missing {line:?} in {err}");
    }
    assert!(err.contains("(7 run, 0 cached)"), "{err}");
}

#[test]
fn off_machine_note_printed_once() {
    let env = FakeEnv::new();
    env.template("templates", "acta", "---\nllm: claude\n---\nMinutes.");
    let fake = ollama("local text");
    let out = env.minuta_llm(
        &[
            "run",
            "clase-4.mp4",
            "-t",
            "acta",
            "-t",
            "summary",
            "-t",
            "acta",
        ],
        &fake,
    );
    out.clone().assert().success();
    let err = stderr(&out);
    let note = "note: acta sends its input to claude (Anthropic)";
    assert_eq!(err.matches(note).count(), 1, "{err}");
    assert!(!err.contains("summary sends"), "{err}");
    let job = env.job_dir("clase-4");
    assert_eq!(
        fs::read_to_string(job.join("acta.md")).unwrap(),
        "fake claude reply\n"
    );
    assert_eq!(fake.chat_count(), 1, "only summary uses Ollama");
}

#[test]
fn run_with_codex_backend_writes_output() {
    let env = FakeEnv::new();
    let fake = ollama("unused");
    let out = env.minuta_llm(
        &[
            "run",
            "clase-4.mp4",
            "-t",
            "summary",
            "--llm",
            "codex:gpt-x",
        ],
        &fake,
    );
    out.clone().assert().success();
    assert!(fake.requests().is_empty(), "no Ollama check for codex");
    let err = stderr(&out);
    assert_eq!(
        err.matches("note: summary sends its input to codex (OpenAI)")
            .count(),
        1,
        "{err}"
    );
    assert!(
        err.contains("[6/6] summary     codex:gpt-x  1 call  "),
        "{err}"
    );
    assert_eq!(
        fs::read_to_string(env.job_dir("clase-4").join("summary.md")).unwrap(),
        "fake codex reply\n"
    );
}

#[test]
fn notice_for_claude_model_still_names_anthropic() {
    let env = FakeEnv::new();
    let fake = ollama("unused");
    let out = env.minuta_llm(
        &[
            "run",
            "clase-4.mp4",
            "-t",
            "summary",
            "--llm",
            "claude:sonnet",
        ],
        &fake,
    );
    out.clone().assert().success();
    let err = stderr(&out);
    assert!(
        err.contains("note: summary sends its input to claude (Anthropic)"),
        "{err}"
    );
    assert!(!err.contains("codex"), "{err}");
}

#[test]
fn llm_flag_overrides_the_backend() {
    let env = FakeEnv::new();
    let fake = ollama("unused");
    let out = env.minuta_llm(
        &["run", "clase-4.mp4", "-t", "summary", "--llm", "claude"],
        &fake,
    );
    out.clone().assert().success();
    assert!(fake.requests().is_empty(), "no Ollama check for claude");
    assert!(stderr(&out).contains("note: summary sends its input to claude (Anthropic)"));
    assert!(stderr(&out).contains("[6/6] summary     claude  1 call  "));
}

#[test]
fn output_lang_flag_reaches_the_prompt() {
    let env = FakeEnv::new();
    let fake = ollama("text");
    env.minuta_llm(
        &["run", "clase-4.mp4", "-t", "summary", "--output-lang", "en"],
        &fake,
    )
    .assert()
    .success();
    let chat = fake
        .requests()
        .into_iter()
        .find(|r| r.path == "/api/chat")
        .unwrap();
    let system = chat.body["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(system.contains("in English"), "{system}");
}

#[test]
fn template_failure_stops_the_run() {
    let env = FakeEnv::new();
    env.template("templates", "a", "Write A.");
    let fake = ollama_with(Arc::new(|_| Reply::error(400, "bad request")));
    let out = env.minuta_llm(&["run", "clase-4.mp4", "-t", "a", "-t", "summary"], &fake);
    out.clone().assert().code(1).stdout("");
    assert!(stderr(&out).contains("bad request"), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("[6/7] a           failed"),
        "{}",
        stderr(&out)
    );
    let job = env.job_dir("clase-4");
    assert_eq!(template_state(&job, "a")["status"], "stale");
    assert_eq!(template_state(&job, "summary"), serde_json::Value::Null);
    assert_eq!(
        fake.chat_count(),
        1,
        "a 400 is not retried and summary never runs"
    );
}
