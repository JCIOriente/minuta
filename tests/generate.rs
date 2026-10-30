use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use minuta::chapters::prompt as chapters_prompt;
use minuta::chapters::{ChaptersSpec, CHAPTERS_VERSION};
use minuta::error::Error;
use minuta::fields::prompt::{fields_block, repair_system, repair_user, schema};
use minuta::fields::{FieldsSpec, Violation, FIELDS_VERSION};
use minuta::generate::{backend_id, output_lang, TemplateRun};
use minuta::job::layout::{derived_dir, template_json};
use minuta::job::{fingerprint, Ctx};
use minuta::llm::call::Policy;
use minuta::llm::chunk::{language_instruction, transcript_text, Event, PROMPT_VERSION};
use minuta::llm::fake::ScriptedBackend;
use minuta::llm::{Backend, BackendId};
use minuta::steps::transcribe::TRANSCRIPT_FILE;
use minuta::template::{Layer, Output, Template};
use minuta::transcript::{from_whisper_json, Segment, Transcript};
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
    let path = dir.join(TRANSCRIPT_FILE);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_string(t).unwrap()).unwrap();
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
        repairs: 2,
        implicit: false,
        implicit_inputs: &[],
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
const FIELDS: &str = "---\noutput: fields\nfields: [title, body]\nlimits:\n  body: { max_chars: 10 }\n---\nWrite about {{job_name}}.\n";

#[test]
fn markdown_artifacts() {
    let t = tpl("summary", TEXT);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    assert_eq!(
        run(&t, &b, "es").artifacts(Path::new("/job")),
        vec![PathBuf::from("/job/summary.md")]
    );
}

#[test]
fn fields_artifacts() {
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    assert_eq!(
        run(&t, &b, "es").artifacts(Path::new("/job")),
        vec![
            PathBuf::from("/job/assets/social.json"),
            PathBuf::from("/job/social.md")
        ]
    );
}

#[test]
fn fields_fingerprint_includes_version() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    let input = fs::read(dir.path().join(TRANSCRIPT_FILE)).unwrap();
    let with_version = |version: u32| {
        fingerprint(&[
            b"template",
            b"social",
            FIELDS.as_bytes(),
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
            version.to_string().as_bytes(),
        ])
    };
    let got = run(&t, &b, "es")
        .fingerprint(&ctx(dir.path()), "reunion")
        .unwrap();
    assert_eq!(got, with_version(FIELDS_VERSION));
    assert_ne!(
        got,
        with_version(FIELDS_VERSION + 1),
        "a new version reruns"
    );
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
fn markdown_fingerprint_pinned() {
    // Recorded on main before fields templates existed: sub-project 2 job folders stay cached.
    let (dir, _) = job();
    let t = tpl("summary", TEXT);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    assert_eq!(
        run(&t, &b, "es")
            .fingerprint(&ctx(dir.path()), "reunion")
            .unwrap(),
        "ca5454c45b4f122fa4df6034c740f2e03507301a9deee312fc2d47d1587755de"
    );
}

#[test]
fn fields_fingerprint_pinned() {
    // Recorded on main before the chapters field existed: sub-project 3 job folders stay cached.
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    assert_eq!(
        run(&t, &b, "es")
            .fingerprint(&ctx(dir.path()), "reunion")
            .unwrap(),
        "617f4ee7afa369b04af79a6655dc2edcf940e7c309b71a619af0f6b004c64b1a"
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

// ---- fields templates ----

const VALID: &str = r#"{"title": "Hola", "body": "Corto."}"#;
const OVER: &str = r#"{"title": "Hola", "body": "Demasiado largo"}"#;
const OVER_VIOLATION: &str =
    "\"body\" has 15 characters; the limit is 10. Shorten it to at most 10 characters.";
const HINT: &str = "raise llm.repairs, relax the limit in a project template, \
                    or use a stronger backend with --llm";

fn fields_spec(t: &Template) -> &FieldsSpec {
    match &t.output {
        Output::Fields(spec) => spec,
        Output::Markdown | Output::Chapters(_) => panic!("{} is not a fields template", t.name),
    }
}

fn execute_events(
    r: &TemplateRun,
    dir: &Path,
) -> (
    minuta::error::Result<minuta::llm::chunk::Generated>,
    Vec<Event>,
) {
    let mut events = Vec::new();
    let result = r.execute(
        &ctx(dir),
        "reunion",
        &policy(),
        1,
        &AtomicBool::new(false),
        &mut |e| events.push(e),
    );
    (result, events)
}

fn assert_no_artifacts(dir: &Path) {
    for path in [template_json(dir, "social"), dir.join("social.md")] {
        assert!(
            !path.try_exists().unwrap(),
            "{} was written",
            path.display()
        );
    }
}

#[test]
fn fields_first_answer_valid() {
    let (dir, transcript) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    let (result, events) = execute_events(&run(&t, &b, "es"), dir.path());
    assert_eq!(result.unwrap().calls, 1);
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(b.schemas(), vec![Some(schema(fields_spec(&t)))]);

    let calls = b.calls();
    let (system, user) = &calls[0];
    let block = fields_block(fields_spec(&t));
    assert!(
        system.ends_with(&format!("{block}\n\n{}", language_instruction("Spanish"))),
        "{system}"
    );
    assert_eq!(user, &transcript_text(&transcript));

    let json = fs::read_to_string(template_json(dir.path(), "social")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        parsed,
        serde_json::json!({
            "version": 1,
            "template": "social",
            "output_lang": "es",
            "fields": {"title": "Hola", "body": "Corto."}
        })
    );
    let md = fs::read_to_string(dir.path().join("social.md")).unwrap();
    assert_eq!(md, "Hola\n\nCorto.\n");
}

#[test]
fn fields_repair_then_valid() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(OVER.into()), Ok(VALID.into())]);
    let (result, events) = execute_events(&run(&t, &b, "es"), dir.path());
    assert_eq!(result.unwrap().calls, 2);
    assert_eq!(
        events,
        vec![Event::Repair {
            attempt: 1,
            total: 2
        }]
    );

    let spec = fields_spec(&t);
    let calls = b.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].0, repair_system(spec, "Spanish"));
    assert_eq!(
        calls[1].1,
        repair_user(OVER, &[Violation(OVER_VIOLATION.into())])
    );
    assert_eq!(b.schemas(), vec![Some(schema(spec)), Some(schema(spec))]);
    let md = fs::read_to_string(dir.path().join("social.md")).unwrap();
    assert_eq!(md, "Hola\n\nCorto.\n");
    assert!(template_json(dir.path(), "social").try_exists().unwrap());
}

#[test]
fn fields_repairs_exhausted_writes_nothing() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(
        ID,
        CONTEXT,
        vec![Ok(OVER.into()), Ok(OVER.into()), Ok(OVER.into())],
    );
    let (result, events) = execute_events(&run(&t, &b, "es"), dir.path());
    let err = result.unwrap_err();
    let want = format!(
        "the answer still breaks 1 limit after 2 repairs: {}; {HINT}",
        OVER_VIOLATION.trim_end_matches('.')
    );
    assert!(
        matches!(err, Error::Template { ref name, ref message }
            if name == "social" && *message == want),
        "{err}"
    );
    assert_eq!(b.calls().len(), 3);
    assert_eq!(
        events,
        vec![
            Event::Repair {
                attempt: 1,
                total: 2
            },
            Event::Repair {
                attempt: 2,
                total: 2
            },
        ]
    );
    assert_no_artifacts(dir.path());
}

#[test]
fn fields_zero_repairs_fails_first() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(
        ID,
        CONTEXT,
        vec![
            Ok(r#"{"body": "Demasiado largo"}"#.into()),
            Ok(VALID.into()),
        ],
    );
    let mut r = run(&t, &b, "es");
    r.repairs = 0;
    let (result, events) = execute_events(&r, dir.path());
    let err = result.unwrap_err();
    let want = format!(
        "the answer still breaks 2 limits after 0 repairs: missing key \"title\"; {}; {HINT}",
        OVER_VIOLATION.trim_end_matches('.')
    );
    assert!(
        matches!(err, Error::Template { ref message, .. } if *message == want),
        "{err}"
    );
    assert_eq!(b.calls().len(), 1);
    assert!(events.is_empty(), "{events:?}");
    assert_no_artifacts(dir.path());
}

#[test]
fn fields_one_repair_message_is_singular() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(OVER.into()), Ok(OVER.into())]);
    let mut r = run(&t, &b, "es");
    r.repairs = 1;
    let (result, _) = execute_events(&r, dir.path());
    let err = result.unwrap_err();
    assert!(
        matches!(err, Error::Template { ref message, .. }
            if message.starts_with("the answer still breaks 1 limit after 1 repair: ")),
        "{err}"
    );
}

#[test]
fn fields_invalid_json_then_valid() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(
        ID,
        CONTEXT,
        vec![Ok("not json at all".into()), Ok(VALID.into())],
    );
    let (result, events) = execute_events(&run(&t, &b, "es"), dir.path());
    assert_eq!(result.unwrap().calls, 2);
    assert_eq!(
        events,
        vec![Event::Repair {
            attempt: 1,
            total: 2
        }]
    );
    let calls = b.calls();
    assert!(
        calls[1].1.starts_with(
            "Previous answer:\nnot json at all\n\nProblems:\n- the answer is not a JSON object ("
        ),
        "{}",
        calls[1].1
    );
    assert!(template_json(dir.path(), "social").try_exists().unwrap());
    assert!(dir.path().join("social.md").try_exists().unwrap());
}

#[test]
fn fields_interrupt_during_repair_writes_nothing() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(OVER.into()), Err(Error::Interrupted)]);
    let (result, _) = execute_events(&run(&t, &b, "es"), dir.path());
    let err = result.unwrap_err();
    assert!(matches!(err, Error::Interrupted), "{err}");
    assert_eq!(b.calls().len(), 2);
    assert_no_artifacts(dir.path());
}

#[test]
fn fields_forced_refreshes() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let forced_run = |b: &ScriptedBackend| {
        let mut r = run(&t, b, "es");
        r.forced = true;
        execute(&r, dir.path()).unwrap()
    };
    // The final call's answer is cached.
    let first = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    execute(&run(&t, &first, "es"), dir.path()).unwrap();

    // Forced: the cached final answer is not read. The repair answer is cached.
    let second = ScriptedBackend::new(ID, CONTEXT, vec![Ok(OVER.into()), Ok(VALID.into())]);
    assert_eq!(forced_run(&second).calls, 2);
    assert_eq!(second.calls().len(), 2, "final call refreshed");

    // Forced again: the cached repair answer is not read either.
    let newer = r#"{"title": "Nuevo", "body": "Breve."}"#;
    let third = ScriptedBackend::new(ID, CONTEXT, vec![Ok(OVER.into()), Ok(newer.into())]);
    assert_eq!(forced_run(&third).calls, 2);
    assert_eq!(third.calls().len(), 2, "repair call refreshed");
    let md = fs::read_to_string(dir.path().join("social.md")).unwrap();
    assert_eq!(md, "Nuevo\n\nBreve.\n");
}

#[test]
fn fields_second_execute_hits_cache() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let first = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    execute(&run(&t, &first, "es"), dir.path()).unwrap();

    let second = ScriptedBackend::new(ID, CONTEXT, vec![]);
    execute(&run(&t, &second, "es"), dir.path()).unwrap();
    assert!(second.calls().is_empty(), "{:?}", second.calls());
}

#[test]
fn fields_chunked_only_final_call_asks_for_json() {
    let dir = TempDir::new().unwrap();
    let segments: Vec<(f64, f64, String)> = (0..60)
        .map(|i| {
            (
                i as f64,
                i as f64 + 1.0,
                format!("Frase numero {i} de la reunion."),
            )
        })
        .collect();
    let segments: Vec<(f64, f64, &str)> = segments
        .iter()
        .map(|(s, e, t)| (*s, *e, t.as_str()))
        .collect();
    write_transcript(dir.path(), &transcript("es", &segments));
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(
        ID,
        800,
        vec![Ok("nota".into()), Ok("nota".into()), Ok(VALID.into())],
    );
    let (result, events) = execute_events(&run(&t, &b, "es"), dir.path());
    assert_eq!(
        events,
        vec![
            Event::Chunk { done: 1, total: 2 },
            Event::Chunk { done: 2, total: 2 },
            Event::Consolidating,
        ]
    );
    assert_eq!(result.unwrap().calls, 3);
    assert_eq!(b.schemas(), vec![None, None, Some(schema(fields_spec(&t)))]);
    let md = fs::read_to_string(dir.path().join("social.md")).unwrap();
    assert_eq!(md, "Hola\n\nCorto.\n");
}

#[test]
fn fields_markdown_write_failure_removes_json() {
    let (dir, _) = job();
    // A non-empty directory where `social.md` goes: renaming the temp file onto it fails.
    fs::create_dir(dir.path().join("social.md")).unwrap();
    fs::write(dir.path().join("social.md").join("keep"), "x").unwrap();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    let err = execute(&run(&t, &b, "es"), dir.path()).unwrap_err();
    assert!(matches!(err, Error::Io(_)), "{err}");
    assert!(
        !template_json(dir.path(), "social").try_exists().unwrap(),
        "social.json left behind"
    );
}

// ---- chapters template ----

const CHAPTERS: &str = "---\noutput: chapters\nlimits:\n  chapters: { max: 20, max_chars: 1500 }\n---\nSplit {{job_name}} into chapters.\n";

/// Six chapters on `class.json`. "La propiedad" at 0:34 is off a segment start: it snaps to
/// the nearest one (32.5 s), floored to 32. The other starts are already floored segment starts.
const CHAPTERS_VALID: &str = r#"{"chapters": [
  {"start": "00:00:00", "title": "Introducción"},
  {"start": "00:00:34", "title": "La propiedad"},
  {"start": "00:01:23", "title": "Movimientos"},
  {"start": "00:02:18", "title": "Referencias y préstamo"},
  {"start": "00:03:25", "title": "El borrow checker"},
  {"start": "00:04:40", "title": "Ejercicios"}
]}"#;
const CHAPTERS_VALID_JSON: &str = r#"{
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
      "start": 83,
      "title": "Movimientos"
    },
    {
      "start": 138,
      "title": "Referencias y préstamo"
    },
    {
      "start": 205,
      "title": "El borrow checker"
    },
    {
      "start": 280,
      "title": "Ejercicios"
    }
  ]
}
"#;
const CHAPTERS_VALID_MD: &str = "0:00 Introducción\n0:32 La propiedad\n1:23 Movimientos\n2:18 Referencias y préstamo\n3:25 El borrow checker\n4:40 Ejercicios\n";
/// The first chapter lasts 4 s: one broken rule.
const CHAPTERS_SHORT_FIRST: &str = r#"{"chapters": [
  {"start": "00:00:00", "title": "Introducción"},
  {"start": "00:00:04", "title": "Presentación"},
  {"start": "00:00:32", "title": "La propiedad"}
]}"#;
const SHORT_FIRST_VIOLATION: &str = "chapter 1 (\"Introducción\") lasts 4 s (0:00 to 0:04); each chapter must last at least 10 s. Merge it with a neighboring chapter or move a start.";

fn chapters_spec(t: &Template) -> &ChaptersSpec {
    match &t.output {
        Output::Chapters(spec) => spec,
        Output::Markdown | Output::Fields(_) => panic!("{} is not the chapters template", t.name),
    }
}

/// A job folder holding the `class.json` transcript (5:35, 25 segments).
fn class_job() -> (TempDir, Transcript) {
    let dir = TempDir::new().unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/whisper/class.json");
    let t = from_whisper_json(&fs::read_to_string(path).unwrap()).unwrap();
    write_transcript(dir.path(), &t);
    (dir, t)
}

fn assert_no_chapters_artifacts(dir: &Path) {
    for path in [template_json(dir, "chapters"), dir.join("chapters.md")] {
        assert!(
            !path.try_exists().unwrap(),
            "{} was written",
            path.display()
        );
    }
}

#[test]
fn chapters_artifacts() {
    let t = tpl("chapters", CHAPTERS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    assert_eq!(
        run(&t, &b, "es").artifacts(Path::new("/job")),
        vec![
            PathBuf::from("/job/assets/chapters.json"),
            PathBuf::from("/job/chapters.md")
        ]
    );
}

#[test]
fn chapters_fingerprint_parts() {
    let (dir, _) = job();
    let t = tpl("chapters", CHAPTERS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    let input = fs::read(dir.path().join(TRANSCRIPT_FILE)).unwrap();
    let with_version = |version: u32| {
        fingerprint(&[
            b"template",
            b"chapters",
            CHAPTERS.as_bytes(),
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
            version.to_string().as_bytes(),
        ])
    };
    let got = run(&t, &b, "es")
        .fingerprint(&ctx(dir.path()), "reunion")
        .unwrap();
    assert_eq!(got, with_version(CHAPTERS_VERSION));
    assert_ne!(
        got,
        with_version(CHAPTERS_VERSION + 1),
        "a new CHAPTERS_VERSION reruns"
    );
}

#[test]
fn chapters_first_answer_valid() {
    let (dir, transcript) = class_job();
    let t = tpl("chapters", CHAPTERS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(CHAPTERS_VALID.into())]);
    let (result, events) = execute_events(&run(&t, &b, "es"), dir.path());
    assert_eq!(result.unwrap().calls, 1);
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(b.schemas(), vec![Some(chapters_prompt::schema())]);

    let calls = b.calls();
    assert_eq!(calls.len(), 1);
    let block = chapters_prompt::chapters_block(chapters_spec(&t));
    assert_eq!(
        calls[0].0,
        format!(
            "Split reunion into chapters.\n\n\n{block}\n\n{}",
            language_instruction("Spanish")
        )
    );
    assert_eq!(calls[0].1, transcript_text(&transcript));

    let json = fs::read_to_string(template_json(dir.path(), "chapters")).unwrap();
    assert_eq!(json, CHAPTERS_VALID_JSON);
    let md = fs::read_to_string(dir.path().join("chapters.md")).unwrap();
    assert_eq!(md, CHAPTERS_VALID_MD);
}

#[test]
fn chapters_repair_then_valid() {
    let (dir, _) = class_job();
    let t = tpl("chapters", CHAPTERS);
    let b = ScriptedBackend::new(
        ID,
        CONTEXT,
        vec![Ok(CHAPTERS_SHORT_FIRST.into()), Ok(CHAPTERS_VALID.into())],
    );
    let (result, events) = execute_events(&run(&t, &b, "es"), dir.path());
    assert_eq!(result.unwrap().calls, 2);
    assert_eq!(
        events,
        vec![Event::Repair {
            attempt: 1,
            total: 2
        }]
    );

    let calls = b.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[1].0,
        chapters_prompt::repair_system(chapters_spec(&t), "Spanish")
    );
    assert_eq!(
        calls[1].1,
        repair_user(
            CHAPTERS_SHORT_FIRST,
            &[Violation(SHORT_FIRST_VIOLATION.into())]
        )
    );
    assert_eq!(
        b.schemas(),
        vec![
            Some(chapters_prompt::schema()),
            Some(chapters_prompt::schema())
        ]
    );
    let json = fs::read_to_string(template_json(dir.path(), "chapters")).unwrap();
    assert_eq!(json, CHAPTERS_VALID_JSON);
    let md = fs::read_to_string(dir.path().join("chapters.md")).unwrap();
    assert_eq!(md, CHAPTERS_VALID_MD);
}

#[test]
fn chapters_repairs_exhausted_writes_nothing() {
    let (dir, _) = class_job();
    let t = tpl("chapters", CHAPTERS);
    let b = ScriptedBackend::new(
        ID,
        CONTEXT,
        vec![
            Ok(CHAPTERS_SHORT_FIRST.into()),
            Ok(CHAPTERS_SHORT_FIRST.into()),
            Ok(CHAPTERS_SHORT_FIRST.into()),
        ],
    );
    let (result, events) = execute_events(&run(&t, &b, "es"), dir.path());
    let err = result.unwrap_err();
    assert_eq!(
        err.to_string(),
        "template chapters: the answer still breaks 1 rule after 2 repairs: chapter 1 \
         (\"Introducción\") lasts 4 s (0:00 to 0:04); each chapter must last at least 10 s. \
         Merge it with a neighboring chapter or move a start; raise llm.repairs, relax the \
         limit in a project template, or use a stronger backend with --llm"
    );
    assert_eq!(b.calls().len(), 3);
    assert_eq!(
        events,
        vec![
            Event::Repair {
                attempt: 1,
                total: 2
            },
            Event::Repair {
                attempt: 2,
                total: 2
            },
        ]
    );
    assert_no_chapters_artifacts(dir.path());
}

#[test]
fn chapters_zero_repairs_counts_rules() {
    let (dir, _) = class_job();
    let t = tpl("chapters", CHAPTERS);
    let two = r#"{"chapters": [
  {"start": "00:00:00", "title": "Introducción"},
  {"start": "00:00:04", "title": "Presentación"}
]}"#;
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(two.into())]);
    let mut r = run(&t, &b, "es");
    r.repairs = 0;
    let (result, events) = execute_events(&r, dir.path());
    let err = result.unwrap_err();
    assert_eq!(
        err.to_string(),
        "template chapters: the answer still breaks 2 rules after 0 repairs: there are 2 \
         chapters; YouTube needs at least 3; chapter 1 (\"Introducción\") lasts 4 s (0:00 to \
         0:04); each chapter must last at least 10 s. Merge it with a neighboring chapter or \
         move a start; raise llm.repairs, relax the limit in a project template, or use a \
         stronger backend with --llm"
    );
    assert_eq!(b.calls().len(), 1);
    assert!(events.is_empty(), "{events:?}");
    assert_no_chapters_artifacts(dir.path());
}

#[test]
fn chapters_short_transcript_no_call() {
    let dir = TempDir::new().unwrap();
    write_transcript(
        dir.path(),
        &transcript("es", &[(0.0, 12.0, "Hola."), (12.0, 25.9, "Adiós.")]),
    );
    let t = tpl("chapters", CHAPTERS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(CHAPTERS_VALID.into())]);
    let err = execute(&run(&t, &b, "es"), dir.path()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "template chapters: the transcript ends at 0:25; YouTube chapters need at least 30 s \
         (3 chapters of at least 10 s). To run a template that declares the chapters field on \
         it, override it in ./templates without that field."
    );
    assert!(b.calls().is_empty(), "{:?}", b.calls());
    assert_no_chapters_artifacts(dir.path());
}

#[test]
fn chapters_transcript_of_30s_runs() {
    let dir = TempDir::new().unwrap();
    write_transcript(
        dir.path(),
        &transcript(
            "es",
            &[
                (0.0, 10.0, "Hola."),
                (10.0, 20.0, "Uno."),
                (20.0, 30.9, "Dos."),
            ],
        ),
    );
    let t = tpl("chapters", CHAPTERS);
    let answer = r#"{"chapters": [
  {"start": "00:00:00", "title": "Inicio"},
  {"start": "00:00:10", "title": "Uno"},
  {"start": "00:00:20", "title": "Dos"}
]}"#;
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(answer.into())]);
    let generated = execute(&run(&t, &b, "es"), dir.path()).unwrap();
    assert_eq!(generated.calls, 1);
    assert_eq!(b.calls().len(), 1);
    let md = fs::read_to_string(dir.path().join("chapters.md")).unwrap();
    assert_eq!(md, "0:00 Inicio\n0:10 Uno\n0:20 Dos\n");
}

#[test]
fn chapters_empty_transcript() {
    let dir = TempDir::new().unwrap();
    write_transcript(dir.path(), &transcript("es", &[]));
    let t = tpl("chapters", CHAPTERS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(CHAPTERS_VALID.into())]);
    let err = execute(&run(&t, &b, "es"), dir.path()).unwrap_err();
    assert_eq!(err.to_string(), "template chapters: transcript is empty");
    assert!(b.calls().is_empty(), "{:?}", b.calls());
    assert_no_chapters_artifacts(dir.path());
}

#[test]
fn chapters_forced_refreshes() {
    let (dir, _) = class_job();
    let t = tpl("chapters", CHAPTERS);
    let forced_run = |b: &ScriptedBackend| {
        let mut r = run(&t, b, "es");
        r.forced = true;
        execute(&r, dir.path()).unwrap()
    };
    // The final call's answer is cached.
    let first = ScriptedBackend::new(ID, CONTEXT, vec![Ok(CHAPTERS_VALID.into())]);
    execute(&run(&t, &first, "es"), dir.path()).unwrap();

    // Forced: the cached final answer is not read. The repair answer is cached.
    let second = ScriptedBackend::new(
        ID,
        CONTEXT,
        vec![Ok(CHAPTERS_SHORT_FIRST.into()), Ok(CHAPTERS_VALID.into())],
    );
    assert_eq!(forced_run(&second).calls, 2);
    assert_eq!(second.calls().len(), 2, "final call refreshed");

    // Forced again: the cached repair answer is not read either.
    let newer = CHAPTERS_VALID.replace("Ejercicios", "Tarea");
    let third = ScriptedBackend::new(
        ID,
        CONTEXT,
        vec![Ok(CHAPTERS_SHORT_FIRST.into()), Ok(newer)],
    );
    assert_eq!(forced_run(&third).calls, 2);
    assert_eq!(third.calls().len(), 2, "repair call refreshed");
    let md = fs::read_to_string(dir.path().join("chapters.md")).unwrap();
    assert_eq!(md, CHAPTERS_VALID_MD.replace("Ejercicios", "Tarea"));
}

// ---- fields templates with the chapters field ----

const YOUTUBE: &str = "---\noutput: fields\nfields: [title, chapters, body]\nlimits:\n  body: { max_chars: 10 }\n---\nWrite about {{job_name}}.\n";
const CHAPTERS_ARTIFACT: &str = "{\n  \"version\": 1,\n  \"output_lang\": \"es\",\n  \"chapters\": [\n    {\n      \"start\": 0,\n      \"title\": \"Introducción\"\n    },\n    {\n      \"start\": 12,\n      \"title\": \"Empezamos\"\n    }\n  ]\n}\n";

fn write_chapters_json(dir: &Path, text: impl AsRef<[u8]>) {
    let path = template_json(dir, "chapters");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// The short job folder plus `assets/chapters.json` holding `text`.
fn youtube_job(text: &str) -> (TempDir, Transcript) {
    let (dir, t) = job();
    write_chapters_json(dir.path(), text);
    (dir, t)
}

fn assert_no_youtube_artifacts(dir: &Path) {
    for path in [template_json(dir, "youtube"), dir.join("youtube.md")] {
        assert!(
            !path.try_exists().unwrap(),
            "{} was written",
            path.display()
        );
    }
}

#[test]
fn fields_with_chapters_fingerprint_parts() {
    let (dir, _) = youtube_job(CHAPTERS_ARTIFACT);
    let t = tpl("youtube", YOUTUBE);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    let input = fs::read(dir.path().join(TRANSCRIPT_FILE)).unwrap();
    let with = |chapters_json: &[u8], version: u32| {
        fingerprint(&[
            b"template",
            b"youtube",
            YOUTUBE.as_bytes(),
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
            FIELDS_VERSION.to_string().as_bytes(),
            &Sha256::digest(chapters_json),
            version.to_string().as_bytes(),
        ])
    };
    let r = run(&t, &b, "es");
    let got = r.fingerprint(&ctx(dir.path()), "reunion").unwrap();
    assert_eq!(got, with(CHAPTERS_ARTIFACT.as_bytes(), CHAPTERS_VERSION));
    assert_ne!(
        got,
        with(CHAPTERS_ARTIFACT.as_bytes(), CHAPTERS_VERSION + 1),
        "a new CHAPTERS_VERSION reruns"
    );

    let other = CHAPTERS_ARTIFACT.replace("Empezamos", "Comenzamos");
    write_chapters_json(dir.path(), &other);
    let changed = r.fingerprint(&ctx(dir.path()), "reunion").unwrap();
    assert_eq!(changed, with(other.as_bytes(), CHAPTERS_VERSION));
    assert_ne!(changed, got, "new chapters.json content reruns");
}

#[test]
fn fields_chapters_fingerprint_json_missing() {
    let (dir, _) = job();
    let t = tpl("youtube", YOUTUBE);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    let err = run(&t, &b, "es")
        .fingerprint(&ctx(dir.path()), "reunion")
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "template youtube: input chapters.json is missing"
    );
}

#[test]
fn fields_filled_from_chapters_json() {
    let (dir, transcript) = youtube_job(CHAPTERS_ARTIFACT);
    let t = tpl("youtube", YOUTUBE);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    let (result, events) = execute_events(&run(&t, &b, "es"), dir.path());
    assert_eq!(result.unwrap().calls, 1);
    assert!(events.is_empty(), "{events:?}");

    // The request is the one for `[title, body]` with the same limits: no trace of chapters.
    let plain = tpl("social", FIELDS);
    assert_eq!(b.schemas(), vec![Some(schema(fields_spec(&plain)))]);
    let calls = b.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].0,
        format!(
            "Write about reunion.\n\n\n{}\n\n{}",
            fields_block(fields_spec(&plain)),
            language_instruction("Spanish")
        )
    );
    assert_eq!(calls[0].1, transcript_text(&transcript));

    let json = fs::read_to_string(template_json(dir.path(), "youtube")).unwrap();
    assert_eq!(
        json,
        "{\n  \"version\": 1,\n  \"template\": \"youtube\",\n  \"output_lang\": \"es\",\n  \
         \"fields\": {\n    \"title\": \"Hola\",\n    \"chapters\": [\n      {\n        \
         \"start\": 0,\n        \"title\": \"Introducción\"\n      },\n      {\n        \
         \"start\": 12,\n        \"title\": \"Empezamos\"\n      }\n    ],\n    \
         \"body\": \"Corto.\"\n  }\n}\n"
    );
    let md = fs::read_to_string(dir.path().join("youtube.md")).unwrap();
    assert_eq!(md, "Hola\n\n0:00 Introducción\n0:12 Empezamos\n\nCorto.\n");
}

#[test]
fn fields_chapters_language_mismatch() {
    let (dir, _) = youtube_job(CHAPTERS_ARTIFACT);
    let t = tpl("youtube", YOUTUBE);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    let err = execute(&run(&t, &b, "en"), dir.path()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "template youtube: chapters.json is in es but youtube writes en; give both templates \
         the same lang, or use --output-lang"
    );
    assert!(b.calls().is_empty(), "{:?}", b.calls());
    assert_no_youtube_artifacts(dir.path());
}

#[test]
fn fields_chapters_json_missing() {
    let (dir, _) = job();
    let t = tpl("youtube", YOUTUBE);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    let err = execute(&run(&t, &b, "es"), dir.path()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "template youtube: input chapters.json is missing"
    );
    assert!(b.calls().is_empty(), "{:?}", b.calls());
    assert_no_youtube_artifacts(dir.path());
}

#[test]
fn fields_chapters_json_version() {
    let (dir, _) = youtube_job(r#"{"version": 2, "output_lang": "es", "chapters": []}"#);
    let t = tpl("youtube", YOUTUBE);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    let err = execute(&run(&t, &b, "es"), dir.path()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "template youtube: chapters.json has version 2, which this version of minuta cannot read"
    );
    assert!(b.calls().is_empty(), "{:?}", b.calls());
    assert_no_youtube_artifacts(dir.path());
}

#[test]
fn fields_chapters_json_damaged() {
    let (dir, _) = youtube_job(r#"{"version": 1, "output_lang": "es"}"#);
    let t = tpl("youtube", YOUTUBE);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    let err = execute(&run(&t, &b, "es"), dir.path()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "template youtube: chapters.json: missing field `chapters`"
    );
    assert!(b.calls().is_empty(), "{:?}", b.calls());
    assert_no_youtube_artifacts(dir.path());
}

#[test]
fn fields_chapters_json_not_utf8() {
    let (dir, _) = job();
    write_chapters_json(dir.path(), [0xff, 0xfe]);
    let t = tpl("youtube", YOUTUBE);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    let err = execute(&run(&t, &b, "es"), dir.path()).unwrap_err();
    assert_eq!(
        err.to_string(),
        "template youtube: chapters.json: invalid utf-8 sequence of 1 bytes from index 0"
    );
    assert!(b.calls().is_empty(), "{:?}", b.calls());
    assert_no_youtube_artifacts(dir.path());
}

#[test]
fn fields_chapters_json_unreadable_is_io_error() {
    let (dir, _) = job();
    fs::create_dir_all(template_json(dir.path(), "chapters")).unwrap();
    let t = tpl("youtube", YOUTUBE);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    let err = execute(&run(&t, &b, "es"), dir.path()).unwrap_err();
    let path = template_json(dir.path(), "chapters");
    assert!(
        matches!(err, Error::Io(ref e) if e.to_string().starts_with(&format!("{}: ", path.display()))),
        "{err}"
    );
    assert!(b.calls().is_empty(), "{:?}", b.calls());
}

// ---- where the outputs go: requested and implicit templates ----

const SOCIAL_JSON: &str = "{\n  \"version\": 1,\n  \"template\": \"social\",\n  \"output_lang\": \"es\",\n  \"fields\": {\n    \"title\": \"Hola\",\n    \"body\": \"Corto.\"\n  }\n}\n";

fn run_implicit<'a>(
    template: &'a Template,
    backend: &'a dyn Backend,
    implicit: bool,
    implicit_inputs: &'a [String],
) -> TemplateRun<'a> {
    TemplateRun {
        implicit,
        implicit_inputs,
        ..run(template, backend, "es")
    }
}

/// Every file under `dir` (relative, sorted), skipping the transcript the job starts with and
/// the call cache.
fn files_under(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, base, out);
            } else {
                out.push(path.strip_prefix(base).unwrap().display().to_string());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.retain(|f| f != TRANSCRIPT_FILE && !f.starts_with(".minuta/llm-cache/"));
    out.sort();
    out
}

#[test]
fn requested_template_writes_md_at_root_and_json_in_assets() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    execute(&run(&t, &b, "es"), dir.path()).unwrap();
    assert_eq!(files_under(dir.path()), ["assets/social.json", "social.md"]);
    assert_eq!(
        fs::read_to_string(template_json(dir.path(), "social")).unwrap(),
        SOCIAL_JSON
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("social.md")).unwrap(),
        "Hola\n\nCorto.\n"
    );
}

#[test]
fn implicit_dependency_writes_to_derived() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    let r = run_implicit(&t, &b, true, &[]);
    execute(&r, dir.path()).unwrap();
    assert_eq!(
        files_under(dir.path()),
        [".minuta/derived/social.json", ".minuta/derived/social.md"]
    );
    let derived = derived_dir(dir.path());
    assert_eq!(
        r.artifacts(dir.path()),
        vec![derived.join("social.json"), derived.join("social.md")]
    );
    assert_eq!(
        fs::read_to_string(derived.join("social.json")).unwrap(),
        SOCIAL_JSON
    );
    assert_eq!(
        fs::read_to_string(derived.join("social.md")).unwrap(),
        "Hola\n\nCorto.\n"
    );
}

#[test]
fn implicit_markdown_dependency_writes_only_md_to_derived() {
    let (dir, _) = job();
    let t = tpl("summary", TEXT);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok("Resumen.".into())]);
    execute(&run_implicit(&t, &b, true, &[]), dir.path()).unwrap();
    assert_eq!(files_under(dir.path()), [".minuta/derived/summary.md"]);
}

#[test]
fn dependent_reads_implicit_inputs_from_derived() {
    let (dir, _) = job();
    let derived = derived_dir(dir.path());
    fs::create_dir_all(&derived).unwrap();
    fs::write(derived.join("chapters.json"), CHAPTERS_ARTIFACT).unwrap();
    fs::write(derived.join("summary.md"), "El resumen.\n").unwrap();
    let implicit = ["chapters".to_string(), "summary".to_string()];

    let youtube = tpl("youtube", YOUTUBE);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    let r = run_implicit(&youtube, &b, false, &implicit);
    execute(&r, dir.path()).unwrap();
    let json = fs::read_to_string(template_json(dir.path(), "youtube")).unwrap();
    assert_eq!(
        json,
        "{\n  \"version\": 1,\n  \"template\": \"youtube\",\n  \"output_lang\": \"es\",\n  \
         \"fields\": {\n    \"title\": \"Hola\",\n    \"chapters\": [\n      {\n        \
         \"start\": 0,\n        \"title\": \"Introducción\"\n      },\n      {\n        \
         \"start\": 12,\n        \"title\": \"Empezamos\"\n      }\n    ],\n    \
         \"body\": \"Corto.\"\n  }\n}\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("youtube.md")).unwrap(),
        "Hola\n\n0:00 Introducción\n0:12 Empezamos\n\nCorto.\n"
    );

    let brief = tpl("brief", "---\ninput: summary\n---\nShorten it.\n");
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok("Corto.".into())]);
    let r = run_implicit(&brief, &b, false, &implicit);
    execute(&r, dir.path()).unwrap();
    assert_eq!(b.calls()[0].1, "El resumen.\n");
    // The same fingerprint as when the inputs sit in their normal places.
    assert_eq!(r.fingerprint(&ctx(dir.path()), "reunion").unwrap(), {
        let (normal, _) = job();
        fs::write(normal.path().join("summary.md"), "El resumen.\n").unwrap();
        run(&brief, &b, "es")
            .fingerprint(&ctx(normal.path()), "reunion")
            .unwrap()
    });
}

#[test]
fn requesting_a_derived_template_moves_its_outputs() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into()), Ok(VALID.into())]);
    execute(&run_implicit(&t, &b, true, &[]), dir.path()).unwrap();
    let requested = run(&t, &b, "es");
    execute(&requested, dir.path()).unwrap();
    assert_eq!(files_under(dir.path()), ["assets/social.json", "social.md"]);
}

#[test]
fn move_derived_moves_files_without_running() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    execute(&run_implicit(&t, &b, true, &[]), dir.path()).unwrap();
    let requested = run(&t, &b, "es");
    assert!(requested.move_derived(&ctx(dir.path())).unwrap());
    assert_eq!(files_under(dir.path()), ["assets/social.json", "social.md"]);
    assert_eq!(
        fs::read_to_string(template_json(dir.path(), "social")).unwrap(),
        SOCIAL_JSON
    );
    assert!(!requested.move_derived(&ctx(dir.path())).unwrap());
    assert_eq!(b.calls().len(), 1, "no new call");
}

#[test]
fn move_derived_removes_a_copy_when_the_normal_file_exists() {
    let (dir, _) = job();
    let t = tpl("summary", TEXT);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok("Resumen.".into())]);
    execute(&run_implicit(&t, &b, true, &[]), dir.path()).unwrap();
    fs::write(dir.path().join("summary.md"), "Resumen.\n").unwrap();
    assert!(run(&t, &b, "es").move_derived(&ctx(dir.path())).unwrap());
    assert_eq!(files_under(dir.path()), ["summary.md"]);
}

#[test]
fn move_derived_handles_each_file_of_a_pair_on_its_own() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    execute(&run_implicit(&t, &b, true, &[]), dir.path()).unwrap();
    // The json already sits in assets/; only the md still has to move.
    fs::create_dir_all(template_json(dir.path(), "social").parent().unwrap()).unwrap();
    fs::write(template_json(dir.path(), "social"), "kept\n").unwrap();
    assert!(run(&t, &b, "es").move_derived(&ctx(dir.path())).unwrap());
    assert_eq!(files_under(dir.path()), ["assets/social.json", "social.md"]);
    assert_eq!(
        fs::read_to_string(template_json(dir.path(), "social")).unwrap(),
        "kept\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("social.md")).unwrap(),
        "Hola\n\nCorto.\n"
    );
}

#[test]
fn derived_copy_that_cannot_be_removed_is_an_error() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    let blocker = derived_dir(dir.path()).join("social.json");
    fs::create_dir_all(&blocker).unwrap();
    let err = execute(&run(&t, &b, "es"), dir.path()).unwrap_err();
    assert!(
        matches!(err, Error::Io(ref e) if e.to_string().starts_with(&format!("{}: ", blocker.display()))),
        "{err}"
    );
}

#[test]
fn derived_copy_that_cannot_be_read_is_an_error() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    let blocker = derived_dir(dir.path()).join("social.json");
    fs::create_dir_all(&blocker).unwrap();
    let err = run(&t, &b, "es")
        .move_derived(&ctx(dir.path()))
        .unwrap_err();
    assert!(
        matches!(err, Error::Io(ref e) if e.to_string().starts_with(&format!("{}: ", blocker.display()))),
        "{err}"
    );
}

/// Replaces `assets/` with a symlink to a new folder that holds the transcript; returns it.
#[cfg(unix)]
fn symlink_assets(dir: &Path) -> TempDir {
    let outside = TempDir::new().unwrap();
    let assets = dir.join("assets");
    fs::rename(
        assets.join("transcript.json"),
        outside.path().join("transcript.json"),
    )
    .unwrap();
    fs::remove_dir_all(&assets).unwrap();
    std::os::unix::fs::symlink(outside.path(), &assets).unwrap();
    outside
}

#[cfg(unix)]
#[test]
fn outputs_refuse_a_symlinked_assets_folder() {
    let (dir, _) = job();
    let outside = symlink_assets(dir.path());
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    assert!(execute(&run(&t, &b, "es"), dir.path()).is_err());
    assert_eq!(files_under(outside.path()), ["transcript.json"]);
    assert!(!dir.path().join("social.md").try_exists().unwrap());
}

#[cfg(unix)]
#[test]
fn moving_derived_files_refuses_a_symlinked_assets_folder() {
    let (dir, _) = job();
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![Ok(VALID.into())]);
    execute(&run_implicit(&t, &b, true, &[]), dir.path()).unwrap();
    let outside = symlink_assets(dir.path());
    assert!(run(&t, &b, "es").move_derived(&ctx(dir.path())).is_err());
    assert_eq!(files_under(outside.path()), ["transcript.json"]);
}
