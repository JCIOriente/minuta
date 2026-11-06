use super::*;

#[test]
fn apply_writes_a_new_file_and_records_it() {
    let d = Dest::new();
    let text = template_text("a");
    let a = entry("alpha", &text);
    let mut f = Fetcher::new(&[("alpha", text.as_bytes())]);
    let mut manifest = Manifest::default();
    let done = run(&d.dir(), &[&a], false, &mut f, &mut manifest).unwrap();
    assert_eq!(
        done,
        vec![Done {
            name: "alpha".into(),
            path: d.dir().join("alpha.md"),
            outcome: Outcome::Created,
        }]
    );
    assert_eq!(fs::read_to_string(d.dir().join("alpha.md")).unwrap(), text);
    assert_eq!(
        manifest.get("alpha"),
        Some(sha256_hex(text.as_bytes()).as_str())
    );
    assert_eq!(
        fs::read_to_string(d.dir().join(".pulled.toml")).unwrap(),
        manifest.to_toml()
    );
}

#[test]
fn apply_creates_the_missing_folder() {
    let d = Dest::new();
    let text = template_text("a");
    let a = entry("alpha", &text);
    let mut f = Fetcher::new(&[("alpha", text.as_bytes())]);
    run(&d.dir(), &[&a], false, &mut f, &mut Manifest::default()).unwrap();
    assert!(d.dir().join("alpha.md").is_file());
}

#[test]
fn apply_leaves_no_temp_files_behind() {
    let d = Dest::new();
    let text = template_text("a");
    let a = entry("alpha", &text);
    let mut f = Fetcher::new(&[("alpha", text.as_bytes())]);
    run(&d.dir(), &[&a], false, &mut f, &mut Manifest::default()).unwrap();
    let mut names: Vec<String> = fs::read_dir(d.dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, [".pulled.toml", "alpha.md"]);
}

#[test]
fn apply_update_available_is_applied_without_force() {
    let d = Dest::new();
    let old = template_text("old");
    let new = template_text("new");
    d.write("alpha.md", &old);
    let a = entry("alpha", &new);
    let mut f = Fetcher::new(&[("alpha", new.as_bytes())]);
    let mut manifest = manifest_with(&[("alpha", &old)]);
    let done = run(&d.dir(), &[&a], false, &mut f, &mut manifest).unwrap();
    assert_eq!(done[0].outcome, Outcome::Updated);
    assert_eq!(fs::read_to_string(d.dir().join("alpha.md")).unwrap(), new);
    assert_eq!(
        manifest.get("alpha"),
        Some(sha256_hex(new.as_bytes()).as_str())
    );
    assert!(!d.dir().join("alpha.md.bak").exists());
}

#[test]
fn apply_modified_is_skipped_without_force() {
    let d = Dest::new();
    let pulled = template_text("pulled");
    let edited = template_text("edited");
    d.write("alpha.md", &edited);
    let a = entry("alpha", &template_text("new"));
    let mut f = Fetcher::new(&[]);
    let mut manifest = manifest_with(&[("alpha", &pulled)]);
    let before = manifest.clone();
    let done = run(&d.dir(), &[&a], false, &mut f, &mut manifest).unwrap();
    assert_eq!(done[0].outcome, Outcome::KeptModified);
    assert_eq!(
        fs::read_to_string(d.dir().join("alpha.md")).unwrap(),
        edited
    );
    assert_eq!(manifest, before);
    assert_eq!(f.calls, Vec::<String>::new());
    assert!(!d.dir().join("alpha.md.bak").exists());
}

#[test]
fn apply_up_to_date_is_not_fetched() {
    let d = Dest::new();
    let text = template_text("a");
    d.write("alpha.md", &text);
    let a = entry("alpha", &text);
    let mut f = Fetcher::new(&[]);
    let mut manifest = manifest_with(&[("alpha", &text)]);
    let done = run(&d.dir(), &[&a], false, &mut f, &mut manifest).unwrap();
    assert_eq!(done[0].outcome, Outcome::KeptUpToDate);
    assert_eq!(f.calls, Vec::<String>::new());
}

#[test]
fn apply_force_on_up_to_date_does_nothing() {
    let d = Dest::new();
    let text = template_text("a");
    d.write("alpha.md", &text);
    let a = entry("alpha", &text);
    let mut f = Fetcher::new(&[]);
    let mut manifest = manifest_with(&[("alpha", &text)]);
    let done = run(&d.dir(), &[&a], true, &mut f, &mut manifest).unwrap();
    assert_eq!(done[0].outcome, Outcome::KeptUpToDate);
    assert_eq!(f.calls, Vec::<String>::new());
    assert!(!d.dir().join("alpha.md.bak").exists());
}
