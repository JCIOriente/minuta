use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use minuta::error::Error;
use minuta::generate::{backend_id, output_lang, TemplateRun};
use minuta::job::{fingerprint, Ctx};
use minuta::llm::call::Policy;
use minuta::llm::chunk::{transcript_text, PROMPT_VERSION};
use minuta::llm::fake::ScriptedBackend;
use minuta::llm::{Backend, BackendId};
use minuta::steps::transcribe::TRANSCRIPT_FILE;
use minuta::template::{Layer, Template};
use minuta::transcript::{Segment, Transcript};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const ID: &str = "ollama:fake";
const CONTEXT: usize = 16384;

fn tpl(name: &str, text: &str) -> Template {
    Template::parse(name, Layer::Project, None, text).expect("template parses")
}

fn ctx(dir: &Path) -> Ctx {
    Ctx {
        dir: dir.to_path_buf(),
        sources: Vec::new(),
        source_digest: String::new(),
    }
}

fn transcript(language: &str, segments: &[(f64, f64, &str)]) -> Transcript {
    Transcript {
        language: language.into(),
        segments: segments
            .iter()
            .map(|&(start, end, text)| Segment {
                start,
                end,
                text: text.into(),
            })
            .collect(),
    }
}

fn write_transcript(dir: &Path, t: &Transcript) {
    fs::write(dir.join(TRANSCRIPT_FILE), serde_json::to_string(t).unwrap()).unwrap();
}

/// A job folder holding a short Spanish transcript.
fn job() -> (TempDir, Transcript) {
    let dir = TempDir::new().unwrap();
    let t = transcript(
        "es",
        &[(0.0, 12.0, "Hola a todos."), (12.0, 40.5, "Empezamos.")],
    );
    write_transcript(dir.path(), &t);
    (dir, t)
}

fn policy() -> Policy {
    Policy {
        retries: 1,
        backoff: Duration::ZERO,
    }
}

fn run<'a>(template: &'a Template, backend: &'a dyn Backend, lang: &str) -> TemplateRun<'a> {
    TemplateRun {
        template,
        backend,
        output_lang: lang.into(),
        forced: false,
    }
}

fn execute(r: &TemplateRun, dir: &Path) -> minuta::error::Result<minuta::llm::chunk::Generated> {
    r.execute(
        &ctx(dir),
        "reunion",
        &policy(),
        1,
        &AtomicBool::new(false),
        &mut |_| {},
    )
}

// ---- output language precedence ----

#[test]
fn output_lang_flag_wins() {
    assert_eq!(
        output_lang("summary", Some("fr"), Some("de"), Some("pt"), "es").unwrap(),
        "fr"
    );
}

#[test]
fn output_lang_template_beats_config() {
    assert_eq!(
        output_lang("summary", None, Some("de"), Some("pt"), "es").unwrap(),
        "de"
    );
}

#[test]
fn output_lang_config_beats_detected() {
    assert_eq!(
        output_lang("summary", None, None, Some("pt"), "es").unwrap(),
        "pt"
    );
}

#[test]
fn output_lang_detected_default() {
    assert_eq!(
        output_lang("summary", None, None, None, "es").unwrap(),
        "es"
    );
}

#[test]
fn output_lang_unknown_detected_fails_the_template() {
    let err = output_lang("summary", None, None, None, "xx").unwrap_err();
    assert!(
        matches!(err, Error::Template { ref name, ref message }
            if name == "summary" && message.contains("'xx'") && message.contains("--output-lang")),
        "{err}"
    );
}

#[test]
fn output_lang_unknown_detected_is_fine_when_another_source_is_set() {
    assert_eq!(
        output_lang("summary", None, None, Some("pt"), "xx").unwrap(),
        "pt"
    );
}

#[test]
fn output_lang_validates_the_chosen_code() {
    for (case, result) in [
        ("flag", output_lang("summary", Some("zz"), None, None, "es")),
        (
            "template",
            output_lang("summary", None, Some("zz"), None, "es"),
        ),
        (
            "config",
            output_lang("summary", None, None, Some("zz"), "es"),
        ),
    ] {
        assert!(
            matches!(result, Err(Error::Usage(ref m)) if m.contains("zz")),
            "{case}: {result:?}"
        );
    }
}

// ---- backend precedence ----

fn id(s: &str) -> BackendId {
    s.parse().unwrap()
}

#[test]
fn backend_flag_wins() {
    let got = backend_id(Some(&id("claude")), Some(&id("ollama:a")), &id("ollama:b"));
    assert_eq!(got.to_string(), "claude");
}

#[test]
fn backend_template_beats_default() {
    let got = backend_id(None, Some(&id("ollama:a")), &id("ollama:b"));
    assert_eq!(got.to_string(), "ollama:a");
}

#[test]
fn backend_default() {
    let got = backend_id(None, None, &id("ollama:b"));
    assert_eq!(got.to_string(), "ollama:b");
}

// ---- fingerprint ----

const TEXT: &str = "Summarize {{job_name}}.\n";

#[test]
fn artifact_is_name_dot_md() {
    let t = tpl("summary", TEXT);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    assert_eq!(run(&t, &b, "es").artifact(), Path::new("summary.md"));
}

#[test]
fn fingerprint_matches_spec_composition() {
    let (dir, _) = job();
    let t = tpl("summary", TEXT);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    let input = fs::read(dir.path().join(TRANSCRIPT_FILE)).unwrap();
    let want = fingerprint(&[
        b"template",
        b"summary",
        TEXT.as_bytes(),
        PROMPT_VERSION.to_string().as_bytes(),
        ID.as_bytes(),
        CONTEXT.to_string().as_bytes(),
        b"es",
        &Sha256::digest(&input),
        b"Spanish",
        b"es",
        b"Spanish",
        b"reunion",
        b"0:00:40",
    ]);
    assert_eq!(
        run(&t, &b, "es")
            .fingerprint(&ctx(dir.path()), "reunion")
            .unwrap(),
        want
    );
}

#[test]
fn fingerprint_sensitive_to_each_part() {
    let (dir, _) = job();
    let base_t = tpl("summary", TEXT);
    let base_b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    let base = run(&base_t, &base_b, "es")
        .fingerprint(&ctx(dir.path()), "reunion")
        .unwrap();

    let renamed = tpl("notes", TEXT);
    let reworded = tpl("summary", &format!("---\ndescription: d\n---\n{TEXT}"));
    let other_id = ScriptedBackend::new("ollama:other", CONTEXT, vec![]);
    let other_context = ScriptedBackend::new(ID, CONTEXT * 2, vec![]);
    let cases: [(&str, &Template, &dyn Backend, &str); 5] = [
        ("name", &renamed, &base_b, "es"),
        ("raw", &reworded, &base_b, "es"),
        ("backend id", &base_t, &other_id, "es"),
        ("context tokens", &base_t, &other_context, "es"),
        ("output language", &base_t, &base_b, "en"),
    ];
    for (case, t, b, lang) in cases {
        let fp = run(t, b, lang)
            .fingerprint(&ctx(dir.path()), "reunion")
            .unwrap();
        assert_ne!(fp, base, "{case}");
    }

    let changed = TempDir::new().unwrap();
    write_transcript(
        changed.path(),
        &transcript("es", &[(0.0, 12.0, "Otra cosa.")]),
    );
    let fp = run(&base_t, &base_b, "es")
        .fingerprint(&ctx(changed.path()), "reunion")
        .unwrap();
    assert_ne!(fp, base, "input artifact content");
}

#[test]
fn fingerprint_template_input_hashes_upstream_markdown() {
    let (dir, _) = job();
    let t = tpl("brief", "---\ninput: summary\n---\nShorten it.\n");
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    let r = run(&t, &b, "es");
    fs::write(dir.path().join("summary.md"), "first").unwrap();
    let first = r.fingerprint(&ctx(dir.path()), "reunion").unwrap();

    // Same language and end time, so the same variables: only the text changed.
    write_transcript(dir.path(), &transcript("es", &[(0.0, 40.5, "Cambio.")]));
    let same = r.fingerprint(&ctx(dir.path()), "reunion").unwrap();
    assert_eq!(same, first, "transcript.json is not the input");

    fs::write(dir.path().join("summary.md"), "second").unwrap();
    let changed = r.fingerprint(&ctx(dir.path()), "reunion").unwrap();
    assert_ne!(changed, first, "upstream markdown changed");
}

#[test]
fn fingerprint_sensitive_to_each_variable() {
    // A chained template: its upstream Markdown stays identical in every case, so only the
    // rendered variables can change the fingerprint.
    let (dir, _) = job();
    fs::write(dir.path().join("summary.md"), "same").unwrap();
    let t = tpl("brief", "---\ninput: summary\n---\nShorten it.\n");
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    let r = run(&t, &b, "es");
    let base = r.fingerprint(&ctx(dir.path()), "reunion").unwrap();

    let cases: [(&str, &str, &str, f64); 3] = [
        ("job_name", "otra", "es", 40.5),
        ("speech_lang", "reunion", "ca", 40.5),
        ("duration", "reunion", "es", 99.0),
    ];
    for (case, job_name, speech, end) in cases {
        let other = TempDir::new().unwrap();
        fs::write(other.path().join("summary.md"), "same").unwrap();
        write_transcript(other.path(), &transcript(speech, &[(0.0, end, "Hola.")]));
        let fp = r.fingerprint(&ctx(other.path()), job_name).unwrap();
        assert_ne!(fp, base, "{case}");
    }
}

#[test]
fn fingerprint_missing_input_is_error() {
    let dir = TempDir::new().unwrap();
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    let cases = [
        (tpl("summary", TEXT), TRANSCRIPT_FILE),
        (
            tpl("brief", "---\ninput: summary\n---\nShorten it.\n"),
            "summary.md",
        ),
    ];
    for (t, file) in &cases {
        let err = run(t, &b, "es")
            .fingerprint(&ctx(dir.path()), "reunion")
            .unwrap_err();
        assert!(
            matches!(err, Error::Template { ref name, ref message }
                if *name == t.name && *message == format!("input {file} is missing")),
            "{file}: {err}"
        );
    }
}

#[test]
fn fingerprint_unreadable_input_is_io_error() {
    let (dir, _) = job();
    fs::create_dir(dir.path().join("summary.md")).unwrap();
    let t = tpl("brief", "---\ninput: summary\n---\nShorten it.\n");
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    let err = run(&t, &b, "es")
        .fingerprint(&ctx(dir.path()), "reunion")
        .unwrap_err();
    assert!(
        matches!(err, Error::Io(ref e) if e.to_string().contains("summary.md")),
        "{err}"
    );
}

// ---- execute ----

#[test]
fn execute_writes_markdown() {
    let (dir, _) = job();
    let t = tpl("summary", TEXT);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok("# Resumen\n\nHola.".into())]);
    let generated = execute(&run(&t, &b, "es"), dir.path()).unwrap();
    assert_eq!(generated.calls, 1);
    assert_eq!(generated.text, "# Resumen\n\nHola.");
    let written = fs::read_to_string(dir.path().join("summary.md")).unwrap();
    assert_eq!(written, "# Resumen\n\nHola.\n");
}

#[test]
fn execute_renders_variables() {
    let dir = TempDir::new().unwrap();
    let t = transcript("ca", &[(0.0, 30.0, "Bon dia."), (3600.0, 3723.9, "Adeu.")]);
    write_transcript(dir.path(), &t);
    let template = tpl(
        "summary",
        "out {{output_lang}} ({{ output_lang_code }}) speech {{speech_lang}} \
         job {{job_name}} dur {{duration}}",
    );
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok("ok".into())]);
    execute(&run(&template, &b, "es"), dir.path()).unwrap();
    let calls = b.calls();
    assert_eq!(calls.len(), 1);
    let (system, user) = &calls[0];
    assert!(
        system.starts_with("out Spanish (es) speech Catalan job reunion dur 1:02:03\n\n"),
        "{system}"
    );
    assert!(
        system.contains("Write the entire response in Spanish"),
        "{system}"
    );
    assert_eq!(user, &transcript_text(&t));
}

#[test]
fn execute_feeds_upstream_markdown() {
    let (dir, _) = job();
    fs::write(dir.path().join("summary.md"), "# Resumen\n\nHola.\n").unwrap();
    let t = tpl("brief", "---\ninput: summary\n---\nShorten it.\n");
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok("Corto.".into())]);
    execute(&run(&t, &b, "es"), dir.path()).unwrap();
    assert_eq!(b.calls()[0].1, "# Resumen\n\nHola.\n");
    let written = fs::read_to_string(dir.path().join("brief.md")).unwrap();
    assert_eq!(written, "Corto.\n");
}

#[test]
fn empty_transcript_fails_without_calls() {
    let dir = TempDir::new().unwrap();
    write_transcript(dir.path(), &transcript("es", &[]));
    let t = tpl("summary", TEXT);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok("unused".into())]);
    let err = execute(&run(&t, &b, "es"), dir.path()).unwrap_err();
    assert!(
        matches!(err, Error::Template { ref name, ref message }
            if name == "summary" && message == "transcript is empty"),
        "{err}"
    );
    assert!(b.calls().is_empty());
    assert!(!dir.path().join("summary.md").try_exists().unwrap());
}

#[test]
fn blank_transcript_fails_without_calls() {
    let dir = TempDir::new().unwrap();
    write_transcript(
        dir.path(),
        &transcript("es", &[(0.0, 5.0, "  "), (5.0, 9.0, "\n")]),
    );
    let t = tpl("summary", TEXT);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok("unused".into())]);
    let err = execute(&run(&t, &b, "es"), dir.path()).unwrap_err();
    assert!(
        matches!(err, Error::Template { ref name, ref message }
            if name == "summary" && message == "transcript is empty"),
        "{err}"
    );
    assert!(b.calls().is_empty());
    assert!(!dir.path().join("summary.md").try_exists().unwrap());
}

#[test]
fn execute_rejects_non_utf8_upstream() {
    let (dir, _) = job();
    fs::write(dir.path().join("summary.md"), [0xff, 0xfe]).unwrap();
    let t = tpl("brief", "---\ninput: summary\n---\nShorten it.\n");
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok("unused".into())]);
    let err = execute(&run(&t, &b, "es"), dir.path()).unwrap_err();
    assert!(
        matches!(err, Error::Template { ref name, ref message }
            if name == "brief" && message.contains("summary.md")),
        "{err}"
    );
    assert!(b.calls().is_empty());
    assert!(!dir.path().join("brief.md").try_exists().unwrap());
}

#[test]
fn generate_error_carries_template_name() {
    let (dir, _) = job();
    let body = "word ".repeat(400);
    let t = tpl("summary", &body);
    let b = ScriptedBackend::new(ID, 200, vec![Ok("unused".into())]);
    let err = execute(&run(&t, &b, "es"), dir.path()).unwrap_err();
    assert!(
        matches!(err, Error::Template { ref name, .. } if name == "summary"),
        "{err}"
    );
    assert!(!dir.path().join("summary.md").try_exists().unwrap());
}

#[test]
fn failed_call_writes_no_markdown() {
    let (dir, _) = job();
    let t = tpl("summary", TEXT);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Err(Error::Interrupted)]);
    let err = execute(&run(&t, &b, "es"), dir.path()).unwrap_err();
    assert!(matches!(err, Error::Interrupted), "{err}");
    assert!(!dir.path().join("summary.md").try_exists().unwrap());
}

#[test]
fn second_execute_hits_cache() {
    let (dir, _) = job();
    let t = tpl("summary", TEXT);
    let first = ScriptedBackend::new(ID, CONTEXT, vec![Ok("Hola.".into())]);
    execute(&run(&t, &first, "es"), dir.path()).unwrap();
    fs::remove_file(dir.path().join("summary.md")).unwrap();

    let second = ScriptedBackend::new(ID, CONTEXT, vec![]);
    let generated = execute(&run(&t, &second, "es"), dir.path()).unwrap();
    assert!(second.calls().is_empty(), "{:?}", second.calls());
    assert_eq!(generated.text, "Hola.");
    let written = fs::read_to_string(dir.path().join("summary.md")).unwrap();
    assert_eq!(written, "Hola.\n");
}

#[test]
fn forced_execute_refreshes_cache() {
    let (dir, _) = job();
    let t = tpl("summary", TEXT);
    let first = ScriptedBackend::new(ID, CONTEXT, vec![Ok("old".into())]);
    execute(&run(&t, &first, "es"), dir.path()).unwrap();

    let forced = ScriptedBackend::new(ID, CONTEXT, vec![Ok("new".into())]);
    let mut r = run(&t, &forced, "es");
    r.forced = true;
    let generated = execute(&r, dir.path()).unwrap();
    assert_eq!(forced.calls().len(), 1);
    assert_eq!(generated.text, "new");
    let written = fs::read_to_string(dir.path().join("summary.md")).unwrap();
    assert_eq!(written, "new\n");

    let after = ScriptedBackend::new(ID, CONTEXT, vec![]);
    let generated = execute(&run(&t, &after, "es"), dir.path()).unwrap();
    assert!(after.calls().is_empty(), "{:?}", after.calls());
    assert_eq!(generated.text, "new", "the forced call overwrote the entry");
}
