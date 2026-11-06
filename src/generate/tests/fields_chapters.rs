//! Fields templates with a `chapters` field, filled from `chapters.json`.

use super::*;

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
