//! Chapters templates: answers, repairs, transcript length and fingerprint.

use super::*;

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
