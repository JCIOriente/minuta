//! Template fingerprints: artifacts, composition and what changes them.

use super::*;

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
    // Recorded on main before fields templates existed: sub-project 2 job folders stay cached. The file is in the 0.9.0 format (no version): such jobs keep their fingerprint.
    let (dir, t0) = job();
    write_transcript_v0_9_0(dir.path(), &t0);
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
    let (dir, t0) = job();
    write_transcript_v0_9_0(dir.path(), &t0);
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
