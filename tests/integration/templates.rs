//! Templates end to end: the real binary, real ffmpeg on a generated clip, the fake whisper,
//! the fake Ollama and the fake `claude`. Every test owns its temp HOME, config and server.

#[path = "../support/fake_ollama.rs"]
mod fake_ollama;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fake_ollama::{FakeOllama, Reply, Request};

use crate::common::*;

const FAKE_CLAUDE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bin/claude");
const NO_OLLAMA: &str = "http://127.0.0.1:9";

fn ollama_with(responder: fake_ollama::Responder) -> FakeOllama {
    FakeOllama::start(
        &["gemma4:26b"],
        serde_json::json!({
            "model_info": {"gemma4.context_length": 32768},
            "capabilities": ["completion"]
        }),
        responder,
    )
}

fn ollama(reply: &'static str) -> FakeOllama {
    ollama_with(Arc::new(move |_| Reply::chat(reply)))
}

fn message(req: &Request, index: usize) -> &str {
    req.body["messages"][index]["content"].as_str().unwrap()
}

fn chats(fake: &FakeOllama) -> Vec<Request> {
    fake.requests()
        .into_iter()
        .filter(|r| r.path == "/api/chat")
        .collect()
}

impl Env {
    /// Writes `<dir>/<name>.md`, where `dir` is relative to the temp dir.
    fn template(&self, dir: &str, name: &str, text: &str) {
        let dir = self.path(dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.md")), text).unwrap();
    }

    /// `command` pointed at `url`, with zero retry backoff and the fake `claude`.
    fn llm_command(&self, args: &[&str], url: &str) -> Command {
        let mut cmd = self.command(args);
        cmd.env("MINUTA_OLLAMA_URL", url)
            .env("MINUTA_LLM_BACKOFF_MS", "0")
            .env("MINUTA_CLAUDE_BIN", FAKE_CLAUDE);
        cmd
    }

    fn run_llm(&self, args: &[&str], fake: &FakeOllama) -> Output {
        self.llm_command(args, &fake.url()).output().unwrap()
    }
}

fn template_status(job: &Path, name: &str) -> serde_json::Value {
    job_json(job)["templates"][name]["status"].clone()
}

/// Waits for `cond` without assuming how long it takes.
fn wait_until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !cond() {
        assert!(Instant::now() < deadline, "{what} never happened");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn env_with_clip() -> Env {
    let env = Env::new();
    env.clip("a.mp4", 440);
    env
}

#[test]
fn templates_summary_is_written_then_cached() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = ollama("the summary");
    let out = env.run_llm(&["run", "a.mp4", "-t", "summary"], &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    let job = env.job_dir("a");
    assert_eq!(
        fs::read_to_string(job.join("summary.md")).unwrap(),
        "the summary\n"
    );
    assert_eq!(template_status(&job, "summary"), "done");
    assert_eq!(fake.chat_count(), 1);

    let out = env.run_llm(&["run", "a.mp4", "-t", "summary"], &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(fake.chat_count(), 1, "the second run makes no chat request");
    assert!(
        cached_steps(&stderr(&out)).contains(&"summary".to_string()),
        "{}",
        stderr(&out)
    );
}

#[test]
fn templates_edit_reruns_only_it_and_its_dependent() {
    require_ffmpeg!();
    let env = env_with_clip();
    env.template("templates", "a", "Write A.");
    env.template("templates", "b", "---\ninput: a\n---\nWrite B.");
    env.template("templates", "c", "Write C.");
    // The reply follows the system prompt, so editing a template changes its output.
    let fake = ollama_with(Arc::new(|req| {
        Reply::chat(&format!("reply to: {}", message(req, 0)))
    }));
    let args = ["run", "a.mp4", "-t", "b", "-t", "c"];
    let out = env.run_llm(&args, &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(fake.chat_count(), 3);

    env.template("templates", "a", "Write A, edited.");
    let out = env.run_llm(&args, &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(fake.chat_count(), 5, "a and b rerun");
    assert_eq!(
        cached_steps(&stderr(&out))
            .into_iter()
            .filter(|s| ["a", "b", "c"].contains(&s.as_str()))
            .collect::<Vec<_>>(),
        ["c"],
        "{}",
        stderr(&out)
    );
}

#[test]
fn templates_chain_reruns_dependent_when_output_differs() {
    require_ffmpeg!();
    let env = env_with_clip();
    env.template("templates", "a", "Write A.");
    env.template("templates", "b", "---\ninput: a\n---\nWrite B.");
    let fake = ollama_with(Arc::new(|req| {
        Reply::chat(&format!("reply to: {}", message(req, 0)))
    }));
    let args = ["run", "a.mp4", "-t", "b"];
    assert!(env.run_llm(&args, &fake).status.success());
    assert_eq!(fake.chat_count(), 2);

    env.template("templates", "a", "Write A differently.");
    let out = env.run_llm(&args, &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(fake.chat_count(), 4, "b reruns on the new output of a");
    assert!(!cached_steps(&stderr(&out)).contains(&"b".to_string()));
}

#[test]
fn templates_chain_keeps_dependent_cached_when_output_is_identical() {
    require_ffmpeg!();
    let env = env_with_clip();
    env.template("templates", "a", "Write A.");
    env.template("templates", "b", "---\ninput: a\n---\nWrite B.");
    let fake = ollama("same text");
    let args = ["run", "a.mp4", "-t", "b"];
    assert!(env.run_llm(&args, &fake).status.success());
    assert_eq!(fake.chat_count(), 2);

    env.template("templates", "a", "Write A differently.");
    let out = env.run_llm(&args, &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(fake.chat_count(), 3, "only a reruns");
    let cached = cached_steps(&stderr(&out));
    assert!(cached.contains(&"b".to_string()), "{}", stderr(&out));
    assert!(!cached.contains(&"a".to_string()), "{}", stderr(&out));
}

#[test]
fn templates_force_makes_new_chat_requests() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = ollama("the summary");
    assert!(env
        .run_llm(&["run", "a.mp4", "-t", "summary"], &fake)
        .status
        .success());
    assert_eq!(fake.chat_count(), 1);

    let out = env.run_llm(
        &["run", "a.mp4", "-t", "summary", "--force", "summary"],
        &fake,
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(fake.chat_count(), 2);
    assert!(!cached_steps(&stderr(&out)).contains(&"summary".to_string()));
}

/// A transcript of `n` segments, 40 s apart, so each is its own line of the prompt input.
fn long_transcript(env: &Env, n: usize) -> std::path::PathBuf {
    let sample = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/whisper/sample.json"
    );
    let mut json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(sample).unwrap()).unwrap();
    let segments: Vec<_> = (0..n)
        .map(|i| {
            serde_json::json!({
                "offsets": {"from": i * 40_000, "to": (i + 1) * 40_000},
                "text": format!(" Segment number {i} of the long recording."),
            })
        })
        .collect();
    json["transcription"] = segments.into();
    let path = env.path("long.json");
    fs::write(&path, json.to_string()).unwrap();
    path
}

#[test]
fn templates_failed_chunk_resumes_with_unfinished_calls_only() {
    require_ffmpeg!();
    let env = env_with_clip();
    env.template("templates", "notes", "Write notes.");
    let fixture = long_transcript(&env, 8);
    // Chat 2 (the third chunk) of the first run fails once with a non-transient 400.
    let fake = ollama_with(Arc::new(|req| match req.chat_index {
        Some(2) => Reply::error(400, "bad chunk"),
        _ => Reply::chat("note"),
    }));
    let configure = |mut cmd: Command| {
        cmd.env("FAKE_WHISPER_FIXTURE", &fixture)
            .env("MINUTA_OLLAMA_NUM_CTX", "190")
            .env("MINUTA_LLM_CONCURRENCY", "1")
            .output()
            .unwrap()
    };
    let args = ["run", "a.mp4", "-t", "notes"];
    let out = configure(env.llm_command(&args, &fake.url()));
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let first = chats(&fake);
    assert_eq!(first.len(), 3, "chunks 1 and 2 done, chunk 3 failed");
    let job = env.job_dir("a");
    assert!(!job.join("notes.md").exists());

    let out = configure(env.llm_command(&args, &fake.url()));
    assert!(out.status.success(), "{}", stderr(&out));
    let all = chats(&fake);
    for r in &all {
        let chars = message(r, 0).chars().count() + message(r, 1).chars().count();
        assert!(chars.div_ceil(3) + 190 / 4 <= 190, "request exceeds budget");
    }
    let second = &all[first.len()..];
    // The first run sent parts 1, 2 and the failed 3; the rerun sends 3 and 4, then the reduce.
    assert!(message(&first[0], 0).contains("part 1 of 4"));
    assert!(message(&first[1], 0).contains("part 2 of 4"));
    assert!(message(&first[2], 0).contains("part 3 of 4"));
    assert_eq!(second.len(), 3, "part 3, part 4 and the reduce");
    assert!(message(&second[0], 0).contains("part 3 of 4"));
    assert!(message(&second[1], 0).contains("part 4 of 4"));
    assert!(message(&second[2], 0).contains("consecutive parts"));
    assert_eq!(fs::read_to_string(job.join("notes.md")).unwrap(), "note\n");
}

#[test]
fn templates_interrupt_during_chat_exits_130_without_output() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = ollama_with(Arc::new(|_| Reply::Hang));
    let mut child = env
        .llm_command(&["run", "a.mp4", "-t", "summary"], &fake.url())
        .spawn()
        .unwrap();
    wait_until("the chat request", || fake.chat_count() >= 1);
    signal("-INT", &child);
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("minuta did not exit after SIGINT");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(130));
    assert!(!env.job_dir("a").join("summary.md").exists());
}

#[test]
fn templates_transient_500_is_retried() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = ollama_with(Arc::new(|req| match req.chat_index {
        Some(0) => Reply::error(500, "overloaded"),
        _ => Reply::chat("recovered"),
    }));
    let out = env.run_llm(&["run", "a.mp4", "-t", "summary"], &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(fake.chat_count(), 2);
    assert_eq!(
        fs::read_to_string(env.job_dir("a").join("summary.md")).unwrap(),
        "recovered\n"
    );
}

#[test]
fn templates_missing_model_exits_1_with_ollama_pull() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = ollama_with(Arc::new(|_| Reply::error(404, "model not found")));
    let out = env.run_llm(&["run", "a.mp4", "-t", "summary"], &fake);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("ollama pull"), "{}", stderr(&out));
    assert_eq!(fake.chat_count(), 1, "404 is not retried");
    assert!(!env.job_dir("a").join("summary.md").exists());
}

#[test]
fn templates_prompt_is_in_the_transcript_language() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = ollama("text");
    let mut cmd = env.llm_command(&["run", "a.mp4", "-t", "summary"], &fake.url());
    let out = cmd.env("MINUTA_OLLAMA_NUM_CTX", "16384").output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let sent = chats(&fake);
    assert!(message(&sent[0], 0).contains("in Spanish"));
    assert_eq!(sent[0].body["options"]["num_ctx"], 16384);
}

#[test]
fn templates_output_lang_flag_overrides_the_language() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = ollama("text");
    let out = env.run_llm(
        &["run", "a.mp4", "-t", "summary", "--output-lang", "en"],
        &fake,
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let sent = chats(&fake);
    assert!(message(&sent[0], 0).contains("in English"));
    assert!(!message(&sent[0], 0).contains("in Spanish"));
}

#[test]
fn templates_output_lang_change_reruns_template() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = ollama("text");
    assert!(env
        .run_llm(&["run", "a.mp4", "-t", "summary"], &fake)
        .status
        .success());
    assert_eq!(fake.chat_count(), 1);
    let out = env.run_llm(
        &["run", "a.mp4", "-t", "summary", "--output-lang", "en"],
        &fake,
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(fake.chat_count(), 2);
    assert!(!cached_steps(&stderr(&out)).contains(&"summary".to_string()));
    assert!(message(&chats(&fake)[1], 0).contains("in English"));
}

#[test]
fn templates_preflight_reports_ollama_and_whisper_together() {
    require_ffmpeg!();
    let env = env_with_clip();
    let mut cmd = env.llm_command(&["run", "a.mp4", "-t", "summary"], NO_OLLAMA);
    let out = cmd
        .env("MINUTA_WHISPER_BIN", env.path("no-whisper"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("  whisper-cli: "), "{err}");
    assert!(err.contains("  ollama: start Ollama"), "{err}");
    assert!(!env.path("minuta-out").exists());
}

#[test]
fn templates_not_requested_contacts_no_backend() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = ollama("unused");
    let out = env.run_llm(&["run", "a.mp4"], &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(fake.requests().is_empty());

    let out = env
        .llm_command(&["run", "a.mp4"], NO_OLLAMA)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
}

#[test]
fn templates_claude_backend_gets_input_on_stdin_and_warns() {
    require_ffmpeg!();
    let env = env_with_clip();
    env.template("templates", "acta", "---\nllm: claude\n---\nMinutes.");
    let stdin = env.path("claude.stdin");
    let fake = ollama("unused");
    let mut cmd = env.llm_command(&["run", "a.mp4", "-t", "acta"], &fake.url());
    let out = cmd
        .env("FAKE_CLAUDE_STDIN", &stdin)
        .env("FAKE_CLAUDE_REPLY", "claude minutes")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(fs::read_to_string(&stdin)
        .unwrap()
        .contains("Buenos días a todos."));
    assert!(
        stderr(&out).contains("acta sends its input to claude (Anthropic)"),
        "{}",
        stderr(&out)
    );
    assert_eq!(
        fs::read_to_string(env.job_dir("a").join("acta.md")).unwrap(),
        "claude minutes\n"
    );
    assert!(fake.requests().is_empty());
}

#[test]
fn templates_cycle_exits_2_naming_it() {
    require_ffmpeg!();
    let env = env_with_clip();
    env.template("templates", "x", "---\ninput: y\n---\nX.");
    env.template("templates", "y", "---\ninput: x\n---\nY.");
    let fake = ollama("unused");
    let out = env.run_llm(&["run", "a.mp4", "-t", "x"], &fake);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("template cycle: x -> y -> x"),
        "{}",
        stderr(&out)
    );
    assert!(!env.path("minuta-out").exists());
}

#[test]
fn templates_list_shows_built_in_user_and_project_layers() {
    let env = Env::new();
    env.template(
        "home/.config/minuta/templates",
        "acta",
        "---\ndescription: Formal minutes\n---\nMinutes.",
    );
    env.template("templates", "notes", "Notes.");
    let out = env.minuta(&["templates", "list"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    let layer = |name: &str| -> String {
        text.lines()
            .find(|l| l.split_whitespace().next() == Some(name))
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or_else(|| panic!("no row for {name} in {text}"))
            .to_string()
    };
    assert_eq!(layer("summary"), "built-in");
    assert_eq!(layer("acta"), "user");
    assert_eq!(layer("notes"), "project");
}

#[test]
fn templates_show_prints_the_file_of_each_layer() {
    let env = Env::new();
    env.template("home/.config/minuta/templates", "acta", "From the user.\n");
    env.template("templates", "notes", "From the project.\n");
    let show = |name: &str| {
        let out = env.minuta(&["templates", "show", name]);
        assert!(out.status.success(), "{}", stderr(&out));
        stdout(&out)
    };
    assert_eq!(show("acta"), "From the user.\n");
    assert_eq!(show("notes"), "From the project.\n");
    assert!(show("summary").contains("Short summary of the recording"));
}
