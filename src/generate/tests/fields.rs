//! Fields templates: answers, repairs and what is written.

use super::*;

const OVER: &str = r#"{"title": "Hola", "body": "Demasiado largo"}"#;
const OVER_VIOLATION: &str =
    "\"body\" has 15 characters; the limit is 10. Shorten it to at most 10 characters.";
const HINT: &str = "raise llm.repairs, relax the limit in a project template, \
                    or use a stronger backend with --llm";

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
