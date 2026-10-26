//! The `chapters` template end to end: the real binary, real ffmpeg on a generated clip, the
//! fake whisper on `class.json` (or `sample.json`), the fake Ollama and the fake `claude`.

use std::fs;
use std::path::Path;
use std::process::Output;
use std::sync::Arc;

use fake_ollama::{FakeOllama, Reply, Request};

use crate::common::*;

const ARGS_CHAPTERS: [&str; 4] = ["run", "a.mp4", "-t", "chapters"];
const ARGS_YOUTUBE: [&str; 4] = ["run", "a.mp4", "-t", "youtube"];

/// The chapters schema sent to Ollama, as `serde_json` writes it (keys sorted).
const SCHEMA: &str = r#"{"properties":{"chapters":{"items":{"properties":{"start":{"type":"string"},"title":{"type":"string"}},"required":["start","title"],"type":"object"},"type":"array"}},"required":["chapters"],"type":"object"}"#;

/// The `chapters.json` of a run that accepted `valid_chapters`.
const VALID_CHAPTERS_JSON: &str = r#"{
  "version": 1,
  "output_lang": "es",
  "chapters": [
    {
      "start": 0,
      "title": "Introducción"
    },
    {
      "start": 32,
      "title": "La propiedad"
    },
    {
      "start": 107,
      "title": "Movimientos y copias"
    },
    {
      "start": 205,
      "title": "Préstamos"
    },
    {
      "start": 280,
      "title": "Ejercicios"
    }
  ]
}
"#;

/// The `chapters.md` of a run that accepted `valid_chapters`.
const VALID_CHAPTERS_MD: &str =
    "0:00 Introducción\n0:32 La propiedad\n1:47 Movimientos y copias\n3:25 Préstamos\n4:40 Ejercicios\n";

/// The rule a `short_first_chapter` answer breaks, as a violation line.
const SHORT_FIRST: &str = "chapter 1 (\"Introducción\") lasts 4 s (0:00 to 0:04); each chapter must last at least 10 s. Merge it with a neighboring chapter or move a start.";

/// What a request is, from its schema and system prompt.
fn role(req: &Request) -> &'static str {
    let system = message(req, 0);
    match (is_chapters(req), has_format(req)) {
        (true, _) if is_repair(req) => "chapters repair",
        (true, _) => "chapters final",
        (false, true) => "youtube",
        (false, false) if system.contains("This is part ") => {
            if system.contains("Divide the recording") {
                "chapters map"
            } else {
                "summary map"
            }
        }
        (false, false) => "text",
    }
}

/// The roles of the chat requests from `from` on, in arrival order.
fn roles(fake: &FakeOllama, from: usize) -> Vec<&'static str> {
    chats(fake).iter().skip(from).map(role).collect()
}

/// How many of `roles` are `role`.
fn count(roles: &[&str], role: &str) -> usize {
    roles.iter().filter(|r| **r == role).count()
}

/// Like `run_class`, with a context so small that the transcript is split into chunks.
fn run_chunked(env: &Env, args: &[&str], fake: &FakeOllama) -> Output {
    class_command(env, args, &fake.url())
        .env("MINUTA_OLLAMA_NUM_CTX", "1200")
        .env("MINUTA_LLM_CONCURRENCY", "1")
        .output()
        .unwrap()
}

/// A chapters answer whose first chapter lasts 4 s (`SHORT_FIRST`).
fn short_first_chapter() -> String {
    chapters_answer(&[
        ("00:00:00", "Introducción"),
        ("00:00:04", "La propiedad"),
        ("00:01:47", "Movimientos y copias"),
        ("00:03:25", "Préstamos"),
    ])
}

/// Fake Ollama that answers chapters requests with `chapters(req)`, the other structured
/// requests with `valid_youtube` and the rest with `text`.
fn scripted(
    chapters: impl Fn(&Request) -> String + Send + Sync + 'static,
    text: &'static str,
) -> FakeOllama {
    ollama_with(Arc::new(move |req| {
        if is_chapters(req) {
            Reply::chat(&chapters(req))
        } else if has_format(req) {
            Reply::chat(&valid_youtube())
        } else {
            Reply::chat(text)
        }
    }))
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Whether `file` exists in `job`; an I/O error other than not-found fails the test.
fn exists(job: &Path, file: &str) -> bool {
    job.join(file).try_exists().unwrap()
}

/// Template steps whose progress line in `stderr` reports a finished run on Ollama, in order.
fn done_templates(stderr: &str) -> Vec<String> {
    stderr
        .lines()
        .filter(|l| l.starts_with('['))
        .filter_map(|l| {
            let words: Vec<&str> = l.split_whitespace().collect();
            (words.get(2).is_some_and(|w| w.starts_with("ollama:"))).then(|| words[1].to_string())
        })
        .collect()
}

fn last_line(out: &Output) -> String {
    stderr(out).lines().last().unwrap_or_default().to_string()
}

#[test]
fn chapters_run_writes_artifacts_then_cached() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = scripted(|_| valid_chapters(), "unused");
    let out = run_class(&env, &ARGS_CHAPTERS, &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    let first = fake.chat_count();
    let out = run_class(&env, &ARGS_CHAPTERS, &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    let job = env.job_dir("a");
    let state = &job_json(&job)["templates"]["chapters"];
    let outcome = (
        read(&job.join("chapters.json")),
        read(&job.join("chapters.md")),
        state["status"].clone(),
        state["artifacts"].clone(),
        first,
        fake.chat_count() - first,
    );
    assert_eq!(
        outcome,
        (
            VALID_CHAPTERS_JSON.to_string(),
            VALID_CHAPTERS_MD.to_string(),
            serde_json::json!("done"),
            serde_json::json!(["chapters.json", "chapters.md"]),
            1,
            0,
        ),
        "{}",
        stderr(&out)
    );
}

#[test]
fn chapters_youtube_runs_in_order_then_cached() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = scripted(|_| valid_chapters(), "the summary");
    let out = run_class(&env, &ARGS_YOUTUBE, &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    let done = done_templates(&stderr(&out));
    let first = fake.chat_count();
    let rerun = run_class(&env, &ARGS_YOUTUBE, &fake);
    assert!(rerun.status.success(), "{}", stderr(&rerun));
    let outcome = (
        done,
        read(&env.job_dir("a").join("youtube.md")),
        first,
        fake.chat_count() - first,
    );
    assert_eq!(
        outcome,
        (
            vec![
                "summary".to_string(),
                "chapters".to_string(),
                "youtube".to_string()
            ],
            VALID_YOUTUBE_MD.to_string(),
            3,
            0,
        ),
        "{}",
        stderr(&out)
    );
}

#[test]
fn chapters_start_off_segment_is_snapped() {
    require_ffmpeg!();
    let env = env_with_clip();
    // 0:05 becomes 0 (first chapter), 0:40 snaps to 0:32.5, 1:50 to 1:47.5, 3:20 to 3:25.5.
    let fake = scripted(
        |_| {
            chapters_answer(&[
                ("00:00:05", "Introducción"),
                ("00:00:40", "La propiedad"),
                ("00:01:50", "Movimientos y copias"),
                ("00:03:20", "Préstamos"),
                ("00:04:40", "Ejercicios"),
            ])
        },
        "unused",
    );
    let out = run_class(&env, &ARGS_CHAPTERS, &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        read(&env.job_dir("a").join("chapters.json")),
        VALID_CHAPTERS_JSON
    );
}

#[test]
fn chapters_broken_rule_then_valid_is_repaired() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = scripted(
        |req| {
            if is_repair(req) {
                valid_chapters()
            } else {
                short_first_chapter()
            }
        },
        "unused",
    );
    let out = run_class(&env, &ARGS_CHAPTERS, &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    let sent = chats(&fake);
    let outcome = (
        sent.iter().map(role).collect::<Vec<_>>(),
        message(&sent[1], 1).to_string(),
    );
    assert_eq!(
        outcome,
        (
            vec!["chapters final", "chapters repair"],
            format!(
                "Previous answer:\n{}\n\nProblems:\n- {SHORT_FIRST}",
                short_first_chapter()
            ),
        )
    );
}

#[test]
fn chapters_always_broken_fails_and_rerun_asks_again() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = scripted(|_| short_first_chapter(), "note");
    let out = run_chunked(&env, &ARGS_YOUTUBE, &fake);
    let job = env.job_dir("a");
    let first_run = (
        out.status.code(),
        last_line(&out),
        exists(&job, "chapters.json"),
        exists(&job, "chapters.md"),
        exists(&job, "youtube.json"),
        exists(&job, "youtube.md"),
    );
    let first_roles = roles(&fake, 0);
    let rerun = run_chunked(&env, &ARGS_YOUTUBE, &fake);
    let rerun_roles = roles(&fake, first_roles.len());
    let by_role = |roles: &[&str]| {
        [
            "summary map",
            "text",
            "chapters map",
            "chapters final",
            "chapters repair",
            "youtube",
        ]
        .map(|r| count(roles, r))
    };
    let message = format!(
        "minuta: template chapters: the answer still breaks 1 rule after 2 repairs: {}; raise llm.repairs, relax the limit in a project template, or use a stronger backend with --llm",
        SHORT_FIRST.trim_end_matches('.')
    );
    let outcome = (
        first_run,
        by_role(&first_roles),
        rerun.status.code(),
        last_line(&rerun),
        rerun_roles,
    );
    assert_eq!(
        outcome,
        (
            (Some(1), message.clone(), false, false, false, false),
            [3, 1, 2, 1, 2, 0],
            Some(1),
            message,
            vec!["chapters final", "chapters repair", "chapters repair"],
        ),
        "{}",
        stderr(&out)
    );
}

#[test]
fn chapters_short_transcript_fails_before_any_call() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = scripted(|_| valid_chapters(), "unused");
    // No FAKE_WHISPER_FIXTURE: the fake whisper emits the 7.25 s `sample.json`.
    let mut cmd = env.command(&ARGS_CHAPTERS);
    let out = cmd
        .env("MINUTA_OLLAMA_URL", fake.url())
        .env("MINUTA_LLM_BACKOFF_MS", "0")
        .output()
        .unwrap();
    assert_eq!(
        (out.status.code(), last_line(&out), fake.chat_count()),
        (
            Some(1),
            "minuta: template chapters: the transcript ends at 0:07; YouTube chapters need at least 30 s (3 chapters of at least 10 s). To run a template that declares the chapters field on it, override it in ./templates without that field.".to_string(),
            0
        )
    );
}

#[test]
fn chapters_chunked_only_consolidation_has_schema() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = scripted(|_| valid_chapters(), "note");
    let out = run_chunked(&env, &ARGS_CHAPTERS, &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    let sent = chats(&fake);
    let format = |r: &Request| serde_json::to_string(&r.body["format"]).unwrap();
    let outcome: Vec<(&str, String)> = sent.iter().map(|r| (role(r), format(r))).collect();
    assert_eq!(
        outcome,
        [
            ("chapters map", "null".to_string()),
            ("chapters map", "null".to_string()),
            ("chapters final", SCHEMA.to_string()),
        ]
    );
}

#[test]
fn chapters_on_claude_gets_block_and_accepts_reply() {
    require_ffmpeg!();
    let env = env_with_clip();
    let argv = env.path("claude.argv");
    let fake = scripted(|_| valid_chapters(), "unused");
    let mut cmd = class_command(
        &env,
        &["run", "a.mp4", "-t", "chapters", "--llm", "claude"],
        &fake.url(),
    );
    let out = cmd
        .env("FAKE_CLAUDE_ARGV", &argv)
        .env("FAKE_CLAUDE_REPLY", valid_chapters())
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let args = read(&argv);
    let system = args
        .split_once("--system-prompt=")
        .and_then(|(_, rest)| rest.split_once("\n--tools\n"))
        .map(|(system, _)| system.to_string())
        .unwrap_or_else(|| panic!("no --system-prompt in {args}"));
    let expected = r#"Divide the recording "a" (0:05:35) into chapters by topic, for the YouTube
description. Start a chapter where the speakers move to a new topic, not at every pause. Each
title names the topic in a few words that a viewer would recognize. Use only topics that are
in the input.


Return only a JSON object with one key, "chapters", and no text before or after it. "chapters" is an array of objects, one per chapter, in the order they happen:
"start" (string): when the chapter starts, copied from a timestamp in the input, like "00:04:12". The first chapter starts at "00:00:00".
"title" (string): the chapter title, on one line, in plain text without Markdown.
Give at least 3 and at most 20 chapters. Each chapter lasts at least 10 seconds. As "0:00 Title" lines, all chapters together have at most 1500 characters.
Keep the keys exactly as written, in English; write the titles in the language given below.

Write the entire response in Spanish, including headings and any placeholder text, regardless of the language of the input or of these instructions."#;
    let outcome = (
        system,
        read(&env.job_dir("a").join("chapters.md")),
        fake.chat_count(),
    );
    assert_eq!(
        outcome,
        (expected.to_string(), VALID_CHAPTERS_MD.to_string(), 0)
    );
}

#[test]
fn chapters_force_reruns_chapters_and_youtube_not_summary() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = scripted(|_| valid_chapters(), "the summary");
    let out = run_class(&env, &ARGS_YOUTUBE, &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    let first = fake.chat_count();
    let out = run_class(
        &env,
        &["run", "a.mp4", "-t", "youtube", "--force", "chapters"],
        &fake,
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        (first, roles(&fake, first)),
        (3, vec!["chapters final", "youtube"]),
        "{}",
        stderr(&out)
    );
}

/// A project `chapters` with the built-in frontmatter and a body that ends with `marker`.
fn chapters_override(marker: &str) -> String {
    format!(
        "---\noutput: chapters\nlimits:\n  chapters: {{ max: 20, max_chars: 1500 }}\n---\nDivide the recording into chapters. {marker}"
    )
}

#[test]
fn chapters_regenerated_identical_keeps_youtube_cached() {
    require_ffmpeg!();
    let env = env_with_clip();
    // The third edit of the template gets a different list (the last chapter dropped).
    let fake = scripted(
        |req| {
            if message(req, 0).contains("Edit 3.") {
                chapters_answer(&[
                    ("00:00:00", "Introducción"),
                    ("00:00:32", "La propiedad"),
                    ("00:01:47", "Movimientos y copias"),
                    ("00:03:25", "Préstamos"),
                ])
            } else {
                valid_chapters()
            }
        },
        "the summary",
    );
    let mut runs = Vec::new();
    for edit in ["Edit 1.", "Edit 2.", "Edit 3."] {
        env.template("templates", "chapters", &chapters_override(edit));
        let before = fake.chat_count();
        let out = run_class(&env, &ARGS_YOUTUBE, &fake);
        assert!(out.status.success(), "{}", stderr(&out));
        runs.push((
            roles(&fake, before),
            done_templates(&stderr(&out)),
            read(&env.job_dir("a").join("youtube.md")),
        ));
    }
    let strings = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    // The youtube prompt does not carry the chapters, so its rerun on new chapters is served
    // by the call cache: it runs again (and fills the new list) without a new request.
    let four_chapters = "Rust ownership\n\nA class about ownership.\n\n0:00 Introducción\n0:32 La propiedad\n1:47 Movimientos y copias\n3:25 Préstamos\n\nrust, ownership\n\nOwnership\n";
    assert_eq!(
        runs,
        [
            (
                vec!["text", "chapters final", "youtube"],
                strings(&["summary", "chapters", "youtube"]),
                VALID_YOUTUBE_MD.to_string()
            ),
            (
                vec!["chapters final"],
                strings(&["chapters"]),
                VALID_YOUTUBE_MD.to_string()
            ),
            (
                vec!["chapters final"],
                strings(&["chapters", "youtube"]),
                four_chapters.to_string()
            ),
        ]
    );
}

#[test]
fn chapters_language_mismatch_fails_youtube() {
    require_ffmpeg!();
    let env = env_with_clip();
    env.template("templates",
        "youtube",
        "---\nlang: en\ninput: summary\noutput: fields\nfields: [title, body, chapters]\n---\nWrite the YouTube copy.",
    );
    let fake = scripted(|_| valid_chapters(), "the summary");
    let out = run_class(&env, &ARGS_YOUTUBE, &fake);
    assert_eq!(
        (out.status.code(), last_line(&out), roles(&fake, 0)),
        (
            Some(1),
            "minuta: template youtube: chapters.json is in es but youtube writes en; give both templates the same lang, or use --output-lang".to_string(),
            vec!["text", "chapters final"],
        )
    );
}

#[test]
fn chapters_input_chapters_receives_chapters_md() {
    require_ffmpeg!();
    let env = env_with_clip();
    env.template(
        "templates",
        "toc",
        "---\ninput: chapters\n---\nWrite the table of contents.",
    );
    let fake = scripted(|_| valid_chapters(), "the table");
    let out = run_class(&env, &["run", "a.mp4", "-t", "toc"], &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    let users: Vec<(&str, String)> = chats(&fake)
        .iter()
        .map(|r| (role(r), message(r, 1).to_string()))
        .collect();
    assert_eq!(users[1], ("text", VALID_CHAPTERS_MD.to_string()));
}

#[test]
fn chapters_templates_list_and_show() {
    let env = Env::new();
    let list = env.minuta(&["templates", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let show = env.minuta(&["templates", "show", "chapters"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let text = stdout(&list);
    let row: Vec<String> = text
        .lines()
        .map(|l| l.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .find(|c| c.first().map(String::as_str) == Some("chapters"))
        .unwrap_or_else(|| panic!("no chapters row in {text}"));
    let built_in = "---
description: Timestamped chapters (YouTube rules)
input: transcript
output: chapters
limits:
  chapters: { max: 20, max_chars: 1500 }
---
Divide the recording \"{{job_name}}\" ({{duration}}) into chapters by topic, for the YouTube
description. Start a chapter where the speakers move to a new topic, not at every pause. Each
title names the topic in a few words that a viewer would recognize. Use only topics that are
in the input.
";
    assert_eq!(
        (row.join(" "), stdout(&show)),
        (
            "chapters built-in transcript chapters (default) Timestamped chapters (YouTube rules)"
                .to_string(),
            built_in.to_string()
        )
    );
}
