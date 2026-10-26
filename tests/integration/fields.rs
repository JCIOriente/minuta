//! Fields templates end to end: the real binary, real ffmpeg on a generated clip, the fake
//! whisper, the fake Ollama (scripted per request) and the fake `claude`.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use fake_ollama::{FakeOllama, Reply, Request};

use crate::common::*;

const BLOCK_INTRO: &str = "Return only a JSON object with exactly these keys";

/// Whether the request is a structured call of the built-in `youtube` template.
fn is_youtube(req: &Request) -> bool {
    has_format(req) && !is_chapters(req)
}

/// A valid answer for the built-in `youtube` template.
fn youtube_answer(title: &str, body: &str) -> String {
    serde_json::json!({
        "title": title,
        "body": body,
        "tags": ["rust", "ownership"],
        "thumbnail_text": "Ownership",
    })
    .to_string()
}

/// The `youtube.json` of a run that answered `valid_youtube` and `valid_chapters`.
fn valid_youtube_json() -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "template": "youtube",
        "output_lang": "es",
        "fields": {
            "title": "Rust ownership",
            "body": "A class about ownership.",
            "chapters": [
                {"start": 0, "title": "Introducción"},
                {"start": 32, "title": "La propiedad"},
                {"start": 107, "title": "Movimientos y copias"},
                {"start": 205, "title": "Préstamos"},
                {"start": 280, "title": "Ejercicios"},
            ],
            "tags": ["rust", "ownership"],
            "thumbnail_text": "Ownership",
        },
    })
}

/// Fake Ollama that answers chapters requests with `valid_chapters`, the other structured
/// requests with `fields` and the rest with a summary.
fn summary_and_fields(fields: impl Fn(&Request) -> String + Send + Sync + 'static) -> FakeOllama {
    ollama_with(Arc::new(move |req| {
        if is_chapters(req) {
            Reply::chat(&valid_chapters())
        } else if has_format(req) {
            Reply::chat(&fields(req))
        } else {
            Reply::chat("the summary")
        }
    }))
}

fn read_json(path: PathBuf) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn fields_run_then_cached() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = summary_and_fields(|_| valid_youtube());
    let out = run_class(&env, &["run", "a.mp4", "-t", "youtube"], &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    let job = env.job_dir("a");
    assert_eq!(
        fs::read_to_string(job.join("summary.md")).unwrap(),
        "the summary\n"
    );
    assert_eq!(read_json(job.join("youtube.json")), valid_youtube_json());
    assert_eq!(
        fs::read_to_string(job.join("youtube.md")).unwrap(),
        VALID_YOUTUBE_MD
    );
    let state = job_json(&job);
    assert_eq!(state["templates"]["youtube"]["status"], "done");
    assert_eq!(
        state["templates"]["youtube"]["artifacts"],
        serde_json::json!(["youtube.json", "youtube.md"])
    );
    assert_eq!(fake.chat_count(), 3, "summary, chapters and youtube");

    let out = run_class(&env, &["run", "a.mp4", "-t", "youtube"], &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(fake.chat_count(), 3, "the second run makes no chat request");
    let cached = cached_steps(&stderr(&out));
    assert_eq!(
        cached,
        [
            "probe",
            "join",
            "audio",
            "transcribe",
            "outputs",
            "summary",
            "chapters",
            "youtube"
        ],
        "{}",
        stderr(&out)
    );
}

#[test]
fn fields_repair_request() {
    require_ffmpeg!();
    let env = env_with_clip();
    let over = youtube_answer("Rust ownership", &"x".repeat(3451));
    let previous = over.clone();
    let fake = summary_and_fields(move |req| {
        if is_repair(req) {
            valid_youtube()
        } else {
            over.clone()
        }
    });
    let out = run_class(&env, &["run", "a.mp4", "-t", "youtube"], &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    let sent = chats(&fake);
    let youtube: Vec<_> = sent.iter().filter(|r| is_youtube(r)).collect();
    assert_eq!(
        sent.len(),
        4,
        "summary, chapters, the final request and one repair"
    );
    assert_eq!(youtube.len(), 2, "the final request and one repair");
    assert!(!is_repair(youtube[0]));
    assert!(is_repair(youtube[1]));
    assert_eq!(
        message(youtube[1], 1),
        format!(
            "Previous answer:\n{previous}\n\nProblems:\n- \"body\" has 3451 characters; the limit is 3450. Shorten it to at most 3450 characters."
        )
    );
    assert!(message(youtube[1], 0).starts_with("You correct a JSON object."));
    let job = env.job_dir("a");
    assert_eq!(read_json(job.join("youtube.json")), valid_youtube_json());
    assert_eq!(
        fs::read_to_string(job.join("youtube.md")).unwrap(),
        VALID_YOUTUBE_MD
    );
}

#[test]
fn fields_repairs_exhausted() {
    require_ffmpeg!();
    let env = env_with_clip();
    let over = youtube_answer("Rust ownership", &"x".repeat(3451));
    let fake = summary_and_fields(move |_| over.clone());
    let args = ["run", "a.mp4", "-t", "youtube"];
    let out = run_class(&env, &args, &fake);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let err = stderr(&out);
    assert_eq!(
        err.lines().last(),
        Some("minuta: template youtube: the answer still breaks 1 limit after 2 repairs: \"body\" has 3451 characters; the limit is 3450. Shorten it to at most 3450 characters; raise llm.repairs, relax the limit in a project template, or use a stronger backend with --llm"),
        "{err}"
    );
    let job = env.job_dir("a");
    assert!(!job.join("youtube.json").exists());
    assert!(!job.join("youtube.md").exists());
    assert_eq!(
        fake.chat_count(),
        5,
        "summary, chapters, the final request, two repairs"
    );

    let out = run_class(&env, &args, &fake);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let sent = chats(&fake);
    assert_eq!(
        sent.len(),
        8,
        "summary and chapters stay cached; youtube asks again"
    );
    assert!(
        !is_repair(&sent[5]),
        "the rerun starts with a new final request"
    );
    assert!(is_repair(&sent[6]) && is_repair(&sent[7]));
}

#[test]
fn fields_invalid_json_repaired() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = summary_and_fields(|req| {
        if is_repair(req) {
            valid_youtube()
        } else {
            "Sure! Here is your copy.".to_string()
        }
    });
    let out = run_class(&env, &["run", "a.mp4", "-t", "youtube"], &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    let sent = chats(&fake);
    let youtube: Vec<_> = sent.iter().filter(|r| is_youtube(r)).collect();
    assert_eq!(
        sent.len(),
        4,
        "summary, chapters, the final request and one repair"
    );
    assert_eq!(youtube.len(), 2);
    assert!(
        message(youtube[1], 1).contains("the answer is not a JSON object"),
        "{}",
        message(youtube[1], 1)
    );
    assert!(message(youtube[1], 1).contains("Sure! Here is your copy."));
    let job = env.job_dir("a");
    assert_eq!(read_json(job.join("youtube.json")), valid_youtube_json());
    assert_eq!(
        fs::read_to_string(job.join("youtube.md")).unwrap(),
        VALID_YOUTUBE_MD
    );
}

#[test]
fn fields_chunked_format_only_final() {
    require_ffmpeg!();
    let env = env_with_clip();
    env.template("templates",
        "headline",
        "---\noutput: fields\nfields: [title]\nlimits:\n  title: { max_chars: 10 }\n---\nWrite a headline.",
    );
    let fixture = long_transcript(&env, 24);
    // The first structured answer is too long, so a repair call follows the consolidation.
    let fake = ollama_with(Arc::new(|req| match (has_format(req), is_repair(req)) {
        (false, _) => Reply::chat("note"),
        (true, false) => Reply::chat(r#"{"title": "A headline that is too long"}"#),
        (true, true) => Reply::chat(r#"{"title": "Short"}"#),
    }));
    let mut cmd = env.llm_command(&["run", "a.mp4", "-t", "headline"], &fake.url());
    let out = cmd
        .env("FAKE_WHISPER_FIXTURE", &fixture)
        .env("MINUTA_OLLAMA_NUM_CTX", "600")
        .env("MINUTA_LLM_CONCURRENCY", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let sent = chats(&fake);
    let by_role = |is_role: &dyn Fn(&str) -> bool| -> Vec<&Request> {
        sent.iter().filter(|r| is_role(message(r, 0))).collect()
    };
    let maps = by_role(&|s| s.contains("This is part "));
    let consolidations = by_role(&|s| s.contains("consecutive parts"));
    let repairs = by_role(&|s| s.starts_with("You correct a JSON object."));
    assert!(maps.len() >= 2, "the input needed several map calls");
    assert_eq!(consolidations.len(), 1);
    assert_eq!(repairs.len(), 1);
    assert_eq!(
        maps.len() + consolidations.len() + repairs.len(),
        sent.len(),
        "every request has one role"
    );
    let schema = serde_json::json!({
        "type": "object",
        "properties": {"title": {"type": "string"}},
        "required": ["title"],
    });
    for map in &maps {
        assert!(map.body["format"].is_null(), "{}", map.body["format"]);
    }
    assert_eq!(consolidations[0].body["format"], schema);
    assert_eq!(repairs[0].body["format"], schema);
    assert_eq!(
        read_json(env.job_dir("a").join("headline.json"))["fields"]["title"],
        "Short"
    );
}

#[test]
fn fields_on_claude() {
    require_ffmpeg!();
    let env = env_with_clip();
    env.template(
        "templates",
        "post",
        "---\nllm: claude\noutput: fields\nfields: [title, tags]\n---\nWrite a post.",
    );
    let argv = env.path("claude.argv");
    let fake = ollama_with(Arc::new(|_| Reply::chat("unused")));
    let mut cmd = env.llm_command(&["run", "a.mp4", "-t", "post"], &fake.url());
    let out = cmd
        .env("FAKE_CLAUDE_ARGV", &argv)
        .env(
            "FAKE_CLAUDE_REPLY",
            r#"{"title": "From claude", "tags": ["a", "b"]}"#,
        )
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let args = fs::read_to_string(&argv).unwrap();
    let (_, after_flag) = args
        .split_once("--system-prompt=")
        .unwrap_or_else(|| panic!("no --system-prompt in {args}"));
    assert!(after_flag.contains(BLOCK_INTRO), "{args}");
    assert!(after_flag.contains("\"title\" (string)"), "{args}");
    assert!(after_flag.contains("\"tags\" (array of strings)"), "{args}");
    let job = env.job_dir("a");
    assert_eq!(
        read_json(job.join("post.json"))["fields"],
        serde_json::json!({"title": "From claude", "tags": ["a", "b"]})
    );
    assert_eq!(
        fs::read_to_string(job.join("post.md")).unwrap(),
        "From claude\n\na, b\n"
    );
    assert!(fake.requests().is_empty());
}

#[test]
fn fields_chained_reads_md() {
    require_ffmpeg!();
    let env = env_with_clip();
    env.template(
        "templates",
        "post",
        "---\ninput: youtube\n---\nWrite the post.",
    );
    let fake = ollama_with(Arc::new(|req| {
        if is_chapters(req) {
            Reply::chat(&valid_chapters())
        } else if has_format(req) {
            Reply::chat(&valid_youtube())
        } else if message(req, 0).contains("Write the post.") {
            Reply::chat("the post")
        } else {
            Reply::chat("the summary")
        }
    }));
    let out = run_class(&env, &["run", "a.mp4", "-t", "post"], &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    let job = env.job_dir("a");
    let youtube_md = fs::read_to_string(job.join("youtube.md")).unwrap();
    assert_eq!(youtube_md, VALID_YOUTUBE_MD);
    assert_eq!(read_json(job.join("youtube.json")), valid_youtube_json());
    let sent = chats(&fake);
    let post: Vec<_> = sent
        .iter()
        .filter(|r| message(r, 0).contains("Write the post."))
        .collect();
    assert_eq!(sent.len(), 4, "summary, chapters, youtube and post");
    assert_eq!(post.len(), 1);
    assert_eq!(message(post[0], 1).trim_end(), youtube_md.trim_end());
    assert_eq!(
        fs::read_to_string(job.join("post.md")).unwrap(),
        "the post\n"
    );
}

/// The built-in `youtube` as a project override with the given title limit.
fn youtube_override(title_max: usize) -> String {
    format!(
        "---\ninput: summary\noutput: fields\nfields: [title, body]\nlimits:\n  title: {{ max_chars: {title_max} }}\n---\nWrite the YouTube copy."
    )
}

#[test]
fn fields_limit_edit_reruns() {
    require_ffmpeg!();
    let env = env_with_clip();
    env.template("templates", "youtube", &youtube_override(100));
    env.template(
        "templates",
        "post",
        "---\ninput: youtube\n---\nWrite the post.",
    );
    env.template("templates", "other", "Write the other.");
    // The title follows the limit in the prompt, so the edit changes youtube.md and
    // the dependent sees a new input.
    let fake = ollama_with(Arc::new(|req| {
        if has_format(req) {
            let title = if message(req, 0).contains("At most 90 characters") {
                "Title 90"
            } else {
                "Title 100"
            };
            Reply::chat(&serde_json::json!({"title": title, "body": "Body"}).to_string())
        } else {
            Reply::chat("text")
        }
    }));
    let args = ["run", "a.mp4", "-t", "post", "-t", "other"];
    let out = env.run_llm(&args, &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(fake.chat_count(), 4, "summary, youtube, post, other");

    env.template("templates", "youtube", &youtube_override(90));
    let out = env.run_llm(&args, &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(fake.chat_count(), 6, "youtube and post rerun");
    let mut cached: Vec<_> = cached_steps(&stderr(&out))
        .into_iter()
        .filter(|s| ["summary", "youtube", "post", "other"].contains(&s.as_str()))
        .collect();
    cached.sort();
    assert_eq!(cached, ["other", "summary"], "{}", stderr(&out));
    assert_eq!(
        read_json(env.job_dir("a").join("youtube.json"))["fields"]["title"],
        "Title 90"
    );
}

#[test]
fn templates_list_output_column() {
    let env = Env::new();
    env.template(
        "templates",
        "reel",
        "---\noutput: fields\nfields: [title]\n---\nWrite a reel.",
    );
    env.template("templates", "notes", "Notes.");
    let out = env.minuta(&["templates", "list"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    let columns = |name: &str| -> Vec<String> {
        text.lines()
            .map(|l| l.split_whitespace().collect::<Vec<_>>())
            .find(|c| c.first() == Some(&name))
            .unwrap_or_else(|| panic!("no row for {name} in {text}"))
            .iter()
            .take(5)
            .map(|c| c.to_string())
            .collect()
    };
    let header = text.lines().next().unwrap();
    assert!(
        header
            .split_whitespace()
            .take(5)
            .eq(["NAME", "LAYER", "INPUT", "OUTPUT", "LLM"]),
        "{header}"
    );
    let rows = [
        ("youtube", ["built-in", "summary", "fields", "(default)"]),
        ("linkedin", ["built-in", "summary", "fields", "(default)"]),
        ("instagram", ["built-in", "summary", "fields", "(default)"]),
        (
            "summary",
            ["built-in", "transcript", "markdown", "(default)"],
        ),
        (
            "chapters",
            ["built-in", "transcript", "chapters", "(default)"],
        ),
        ("reel", ["project", "transcript", "fields", "(default)"]),
        ("notes", ["project", "transcript", "markdown", "(default)"]),
    ];
    for (name, rest) in rows {
        let mut expected = vec![name.to_string()];
        expected.extend(rest.iter().map(|s| s.to_string()));
        assert_eq!(columns(name), expected, "row for {name} in {text}");
    }
}
