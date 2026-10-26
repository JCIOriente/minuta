use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use minuta::error::Error;
use minuta::fields::prompt::{fields_block, repair_system, repair_user, schema};
use minuta::fields::{FieldsSpec, Violation, FIELDS_VERSION};
use minuta::generate::{backend_id, output_lang, TemplateRun};
use minuta::job::{fingerprint, Ctx};
use minuta::llm::call::Policy;
use minuta::llm::chunk::{language_instruction, transcript_text, Event, PROMPT_VERSION};
use minuta::llm::fake::ScriptedBackend;
use minuta::llm::{Backend, BackendId};
use minuta::steps::transcribe::TRANSCRIPT_FILE;
use minuta::template::{Layer, Output, Template};
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
        repairs: 2,
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
        run(&t, &b, "es").artifacts(),
        vec![PathBuf::from("summary.md")]
    );
}

#[test]
fn fields_artifacts() {
    let t = tpl("social", FIELDS);
    let b = ScriptedBackend::new(ID, CONTEXT, vec![]);
    assert_eq!(
        run(&t, &b, "es").artifacts(),
        vec![PathBuf::from("social.json"), PathBuf::from("social.md")]
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
fn markdown_fingerprint_unchanged() {
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
    for file in ["social.json", "social.md"] {
        assert!(!dir.join(file).try_exists().unwrap(), "{file} was written");
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

    let json = fs::read_to_string(dir.path().join("social.json")).unwrap();
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
    assert!(dir.path().join("social.json").try_exists().unwrap());
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
    assert!(dir.path().join("social.json").try_exists().unwrap());
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
        !dir.path().join("social.json").try_exists().unwrap(),
        "social.json left behind"
    );
}
