use super::*;

#[test]
fn apply_force_saves_a_backup_then_overwrites() {
    let d = Dest::new();
    let pulled = template_text("pulled");
    let edited = template_text("edited");
    let new = template_text("new");
    d.write("alpha.md", &edited);
    let a = entry("alpha", &new);
    let mut f = Fetcher::new(&[("alpha", new.as_bytes())]);
    let mut manifest = manifest_with(&[("alpha", &pulled)]);
    let done = run(&d.dir(), &[&a], true, &mut f, &mut manifest).unwrap();
    assert_eq!(done[0].outcome, Outcome::Overwrote);
    assert_eq!(fs::read_to_string(d.dir().join("alpha.md")).unwrap(), new);
    assert_eq!(
        fs::read_to_string(d.dir().join("alpha.md.bak")).unwrap(),
        edited
    );
    assert_eq!(
        manifest.get("alpha"),
        Some(sha256_hex(new.as_bytes()).as_str())
    );
}

#[test]
fn apply_force_overwrites_a_file_minuta_never_wrote_after_backing_it_up() {
    let d = Dest::new();
    let mine = template_text("mine");
    let new = template_text("new");
    d.write("alpha.md", &mine);
    let a = entry("alpha", &new);
    let mut f = Fetcher::new(&[("alpha", new.as_bytes())]);
    let done = run(&d.dir(), &[&a], true, &mut f, &mut Manifest::default()).unwrap();
    assert_eq!(done[0].outcome, Outcome::Overwrote);
    assert_eq!(
        fs::read_to_string(d.dir().join("alpha.md.bak")).unwrap(),
        mine
    );
}

#[test]
fn apply_force_replaces_an_older_backup() {
    let d = Dest::new();
    let edited = template_text("edited");
    d.write("alpha.md", &edited);
    d.write("alpha.md.bak", "an older backup");
    let new = template_text("new");
    let a = entry("alpha", &new);
    let mut f = Fetcher::new(&[("alpha", new.as_bytes())]);
    run(&d.dir(), &[&a], true, &mut f, &mut Manifest::default()).unwrap();
    assert_eq!(
        fs::read_to_string(d.dir().join("alpha.md.bak")).unwrap(),
        edited
    );
}

#[test]
fn apply_force_backup_failure_writes_nothing() {
    let d = Dest::new();
    let edited = template_text("edited");
    d.write("alpha.md", &edited);
    let bak = d.dir().join("alpha.md.bak");
    fs::create_dir(&bak).unwrap(); // a backup cannot replace a directory
    let new_alpha = template_text("new alpha");
    let new_beta = template_text("new beta");
    let a = entry("alpha", &new_alpha);
    let b = entry("beta", &new_beta);
    let mut f = Fetcher::new(&[
        ("alpha", new_alpha.as_bytes()),
        ("beta", new_beta.as_bytes()),
    ]);
    let mut manifest = Manifest::default();
    let text = err_text(run(&d.dir(), &[&b, &a], true, &mut f, &mut manifest));
    assert!(
        text.starts_with(&format!(
            "I/O error: cannot save a backup to {}: ",
            bak.display()
        )),
        "{text}"
    );
    assert_eq!(
        fs::read_to_string(d.dir().join("alpha.md")).unwrap(),
        edited
    );
    assert!(!d.dir().join("beta.md").exists());
    assert!(!d.dir().join(".pulled.toml").exists());
    assert_eq!(manifest, Manifest::default());
}

#[test]
fn apply_force_backs_up_every_modified_file() {
    let d = Dest::new();
    let pulled = template_text("pulled");
    let (edit_a, edit_b) = (template_text("edited alpha"), template_text("edited beta"));
    let (new_a, new_b) = (template_text("new alpha"), template_text("new beta"));
    d.write("alpha.md", &edit_a);
    d.write("beta.md", &edit_b);
    let a = entry("alpha", &new_a);
    let b = entry("beta", &new_b);
    let mut f = Fetcher::new(&[("alpha", new_a.as_bytes()), ("beta", new_b.as_bytes())]);
    let mut manifest = manifest_with(&[("alpha", &pulled), ("beta", &pulled)]);
    run(&d.dir(), &[&a, &b], true, &mut f, &mut manifest).unwrap();
    assert_eq!(
        fs::read_to_string(d.dir().join("alpha.md.bak")).unwrap(),
        edit_a
    );
    assert_eq!(
        fs::read_to_string(d.dir().join("beta.md.bak")).unwrap(),
        edit_b
    );
    assert_eq!(fs::read_to_string(d.dir().join("alpha.md")).unwrap(), new_a);
    assert_eq!(fs::read_to_string(d.dir().join("beta.md")).unwrap(), new_b);
}

#[test]
fn apply_force_backup_failure_on_the_second_item_writes_nothing() {
    let d = Dest::new();
    let (edit_a, edit_b) = (template_text("edited alpha"), template_text("edited beta"));
    d.write("alpha.md", &edit_a);
    d.write("beta.md", &edit_b);
    let bak = d.dir().join("beta.md.bak");
    fs::create_dir(&bak).unwrap();
    let (new_a, new_b) = (template_text("new alpha"), template_text("new beta"));
    let a = entry("alpha", &new_a);
    let b = entry("beta", &new_b);
    let mut f = Fetcher::new(&[("alpha", new_a.as_bytes()), ("beta", new_b.as_bytes())]);
    let mut manifest = Manifest::default();
    let text = err_text(run(&d.dir(), &[&a, &b], true, &mut f, &mut manifest));
    assert!(
        text.starts_with(&format!(
            "I/O error: cannot save a backup to {}: ",
            bak.display()
        )),
        "{text}"
    );
    assert_eq!(
        fs::read_to_string(d.dir().join("alpha.md")).unwrap(),
        edit_a
    );
    assert_eq!(fs::read_to_string(d.dir().join("beta.md")).unwrap(), edit_b);
    assert!(!d.dir().join(".pulled.toml").exists());
    assert_eq!(manifest, Manifest::default());
}

#[test]
fn apply_verification_failure_with_force_leaves_backups_alone() {
    let d = Dest::new();
    let edited = template_text("edited");
    d.write("alpha.md", &edited);
    d.write("alpha.md.bak", "an older backup");
    let new_a = template_text("new alpha");
    let a = entry("alpha", &new_a);
    let b = entry("beta", &template_text("expected"));
    let mut f = Fetcher::new(&[
        ("alpha", new_a.as_bytes()),
        ("beta", template_text("tampered").as_bytes()),
    ]);
    let text = err_text(run(
        &d.dir(),
        &[&a, &b],
        true,
        &mut f,
        &mut Manifest::default(),
    ));
    assert!(
        text.starts_with("template beta: downloaded file does not match the catalog checksum"),
        "{text}"
    );
    assert_eq!(
        fs::read_to_string(d.dir().join("alpha.md")).unwrap(),
        edited
    );
    assert_eq!(
        fs::read_to_string(d.dir().join("alpha.md.bak")).unwrap(),
        "an older backup"
    );
}

#[test]
fn apply_fetch_error_with_force_leaves_backups_alone() {
    let d = Dest::new();
    let edited = template_text("edited");
    d.write("alpha.md", &edited);
    d.write("alpha.md.bak", "an older backup");
    let new_a = template_text("new alpha");
    let a = entry("alpha", &new_a);
    let b = entry("beta", &template_text("beta"));
    let mut f = Fetcher::new(&[("alpha", new_a.as_bytes())]); // beta has no file
    let text = err_text(run(
        &d.dir(),
        &[&a, &b],
        true,
        &mut f,
        &mut Manifest::default(),
    ));
    assert_eq!(text, "I/O error: no fake file for beta");
    assert_eq!(
        fs::read_to_string(d.dir().join("alpha.md")).unwrap(),
        edited
    );
    assert_eq!(
        fs::read_to_string(d.dir().join("alpha.md.bak")).unwrap(),
        "an older backup"
    );
}
