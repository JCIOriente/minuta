use super::*;

#[test]
fn apply_manifest_changes_only_for_files_written() {
    let d = Dest::new();
    let other = template_text("other");
    let pulled = template_text("pulled");
    let edited = template_text("edited");
    d.write("edited.md", &edited);
    let fresh = template_text("fresh");
    let e_edited = entry("edited", &template_text("upstream"));
    let e_fresh = entry("fresh", &fresh);
    let mut f = Fetcher::new(&[("fresh", fresh.as_bytes())]);
    let mut manifest = manifest_with(&[("other", &other), ("edited", &pulled)]);
    run(
        &d.dir(),
        &[&e_edited, &e_fresh],
        false,
        &mut f,
        &mut manifest,
    )
    .unwrap();
    assert_eq!(
        manifest,
        manifest_with(&[("other", &other), ("edited", &pulled), ("fresh", &fresh)])
    );
    assert_eq!(
        fs::read_to_string(d.dir().join(".pulled.toml")).unwrap(),
        manifest.to_toml()
    );
}

#[test]
fn apply_nothing_written_leaves_the_manifest_file_alone() {
    let d = Dest::new();
    let text = template_text("a");
    d.write("alpha.md", &text);
    let m = manifest_with(&[("alpha", &text)]);
    d.write(".pulled.toml", "# my own comment\nversion = 1\n");
    let a = entry("alpha", &text);
    let mut f = Fetcher::new(&[]);
    let mut manifest = m.clone();
    run(&d.dir(), &[&a], false, &mut f, &mut manifest).unwrap();
    assert_eq!(
        fs::read_to_string(d.dir().join(".pulled.toml")).unwrap(),
        "# my own comment\nversion = 1\n"
    );
}

#[test]
fn an_invalid_manifest_makes_every_file_modified_and_nothing_is_overwritten() {
    let d = Dest::new();
    let mine = template_text("mine");
    d.write("alpha.md", &mine);
    d.write(".pulled.toml", "version = [");
    let a = entry("alpha", &template_text("upstream"));
    let (mut manifest, warning) = read_manifest(&d.dir()).unwrap();
    assert!(warning.is_some());
    let mut f = Fetcher::new(&[]);
    let done = run(&d.dir(), &[&a], false, &mut f, &mut manifest).unwrap();
    assert_eq!(done[0].outcome, Outcome::KeptModified);
    assert_eq!(fs::read_to_string(d.dir().join("alpha.md")).unwrap(), mine);
}

#[test]
fn apply_write_failure_keeps_the_manifest_truthful() {
    let d = Dest::new();
    let dir = d.create();
    let first = template_text("first");
    let second = template_text("second");
    // A directory where the temp file goes makes the second write fail after the first
    // one has landed.
    fs::create_dir(dir.join("second.md.tmp")).unwrap();
    let a = entry("first", &first);
    let b = entry("second", &second);
    let mut f = Fetcher::new(&[("first", first.as_bytes()), ("second", second.as_bytes())]);
    let mut manifest = Manifest::default();
    let text = err_text(run(&dir, &[&a, &b], false, &mut f, &mut manifest));
    assert!(
        text.starts_with(&format!(
            "I/O error: cannot write {}: ",
            dir.join("second.md").display()
        )),
        "{text}"
    );
    assert!(text.ends_with("; files written: first"), "{text}");
    assert_eq!(fs::read_to_string(dir.join("first.md")).unwrap(), first);
    assert!(!dir.join("second.md").exists());
    assert_eq!(manifest, manifest_with(&[("first", &first)]));
    assert_eq!(
        fs::read_to_string(dir.join(".pulled.toml")).unwrap(),
        manifest.to_toml()
    );
}

#[test]
fn apply_manifest_write_failure_is_an_error() {
    let d = Dest::new();
    let dir = d.create();
    let manifest_tmp = dir.join(".pulled.toml.tmp");
    fs::create_dir(&manifest_tmp).unwrap();
    let text = template_text("a");
    let a = entry("alpha", &text);
    let mut f = Fetcher::new(&[("alpha", text.as_bytes())]);
    let msg = err_text(run(&dir, &[&a], false, &mut f, &mut Manifest::default()));
    assert!(
        msg.starts_with(&format!(
            "I/O error: cannot write {}: ",
            dir.join(".pulled.toml").display()
        )),
        "{msg}"
    );
    assert!(msg.ends_with("; files written: alpha"), "{msg}");
    assert_eq!(fs::read_to_string(dir.join("alpha.md")).unwrap(), text);
}

#[test]
fn apply_write_failure_stops_at_the_first_failure() {
    let d = Dest::new();
    let dir = d.create();
    let (t1, t2, t3) = (template_text("1"), template_text("2"), template_text("3"));
    fs::create_dir(dir.join("second.md.tmp")).unwrap();
    let e1 = entry("first", &t1);
    let e2 = entry("second", &t2);
    let e3 = entry("third", &t3);
    let mut f = Fetcher::new(&[
        ("first", t1.as_bytes()),
        ("second", t2.as_bytes()),
        ("third", t3.as_bytes()),
    ]);
    let mut manifest = Manifest::default();
    err_text(run(&dir, &[&e1, &e2, &e3], false, &mut f, &mut manifest));
    assert!(dir.join("first.md").is_file());
    assert!(!dir.join("second.md").exists());
    assert!(!dir.join("third.md").exists());
    assert_eq!(manifest, manifest_with(&[("first", &t1)]));
}

#[test]
fn apply_reports_both_failures_when_the_manifest_write_also_fails() {
    let d = Dest::new();
    let dir = d.create();
    let (t1, t2) = (template_text("1"), template_text("2"));
    fs::create_dir(dir.join("second.md.tmp")).unwrap();
    fs::create_dir(dir.join(".pulled.toml.tmp")).unwrap();
    let e1 = entry("first", &t1);
    let e2 = entry("second", &t2);
    let mut f = Fetcher::new(&[("first", t1.as_bytes()), ("second", t2.as_bytes())]);
    let msg = err_text(run(
        &dir,
        &[&e1, &e2],
        false,
        &mut f,
        &mut Manifest::default(),
    ));
    assert!(
        msg.starts_with(&format!(
            "I/O error: cannot write {}: ",
            dir.join("second.md").display()
        )),
        "{msg}"
    );
    assert!(
        msg.contains(&format!(
            "; files written: first; also cannot update {}: ",
            dir.join(".pulled.toml").display()
        )),
        "{msg}"
    );
}
