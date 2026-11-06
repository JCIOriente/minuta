//! Executing Markdown templates: output, inputs, failures and the call cache.

use super::*;

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
