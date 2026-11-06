use super::*;

#[test]
fn plan_new_when_the_file_is_missing() {
    let d = Dest::new();
    d.create();
    let a = entry("alpha", &template_text("a"));
    let items = plan(&d.dir(), &[&a], &Manifest::default()).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].entry, &a);
    assert_eq!(items[0].path, d.dir().join("alpha.md"));
    assert_eq!(items[0].state, State::New);
}

#[test]
fn plan_new_when_the_folder_is_missing() {
    let d = Dest::new();
    let a = entry("alpha", &template_text("a"));
    let items = plan(&d.dir(), &[&a], &Manifest::default()).unwrap();
    assert_eq!(items[0].state, State::New);
}

#[test]
fn plan_up_to_date() {
    let d = Dest::new();
    let text = template_text("a");
    d.write("alpha.md", &text);
    let a = entry("alpha", &text);
    let items = plan(&d.dir(), &[&a], &manifest_with(&[("alpha", &text)])).unwrap();
    assert_eq!(items[0].state, State::UpToDate);
}

#[test]
fn plan_update_available() {
    let d = Dest::new();
    let old = template_text("old");
    d.write("alpha.md", &old);
    let a = entry("alpha", &template_text("new"));
    let items = plan(&d.dir(), &[&a], &manifest_with(&[("alpha", &old)])).unwrap();
    assert_eq!(items[0].state, State::UpdateAvailable);
}

#[test]
fn plan_modified_when_the_file_was_edited() {
    let d = Dest::new();
    let pulled = template_text("pulled");
    d.write("alpha.md", &template_text("edited by the user"));
    let a = entry("alpha", &template_text("new"));
    let items = plan(&d.dir(), &[&a], &manifest_with(&[("alpha", &pulled)])).unwrap();
    assert_eq!(items[0].state, State::Modified);
}

#[test]
fn plan_modified_when_minuta_never_wrote_the_file() {
    let d = Dest::new();
    d.write("alpha.md", &template_text("mine"));
    let a = entry("alpha", &template_text("new"));
    let items = plan(&d.dir(), &[&a], &Manifest::default()).unwrap();
    assert_eq!(items[0].state, State::Modified);
}

#[test]
fn plan_keeps_selection_order() {
    let d = Dest::new();
    let a = entry("alpha", &template_text("a"));
    let b = entry("beta", &template_text("b"));
    let items = plan(&d.dir(), &[&b, &a], &Manifest::default()).unwrap();
    let names: Vec<&str> = items.iter().map(|i| i.entry.name.as_str()).collect();
    assert_eq!(names, ["beta", "alpha"]);
}

#[test]
fn plan_destination_that_is_a_file_is_an_error() {
    let d = Dest::new();
    fs::write(d.dir(), "not a folder").unwrap();
    let a = entry("alpha", &template_text("a"));
    assert_eq!(
        err_text(plan(&d.dir(), &[&a], &Manifest::default())),
        format!(
            "I/O error: {} exists and is not a directory",
            d.dir().display()
        )
    );
}

#[cfg(unix)]
#[test]
fn plan_destination_that_is_a_symlink_is_refused() {
    let d = Dest::new();
    let real = d.tmp.path().join("real");
    fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, d.dir()).unwrap();
    let a = entry("alpha", &template_text("a"));
    assert_eq!(
        err_text(plan(&d.dir(), &[&a], &Manifest::default())),
        format!(
            "I/O error: {} is a symlink; refusing to use it",
            d.dir().display()
        )
    );
}

#[cfg(unix)]
#[test]
fn plan_template_that_is_a_symlink_is_refused() {
    let d = Dest::new();
    let dir = d.create();
    let target = d.outside("elsewhere.md", "x");
    let path = dir.join("alpha.md");
    std::os::unix::fs::symlink(&target, &path).unwrap();
    let a = entry("alpha", &template_text("a"));
    assert_eq!(
        err_text(plan(&dir, &[&a], &Manifest::default())),
        format!(
            "I/O error: {} is a symlink; refusing to use it",
            path.display()
        )
    );
}

#[test]
fn plan_template_that_is_a_directory_is_an_error() {
    let d = Dest::new();
    let dir = d.create();
    let path = dir.join("alpha.md");
    fs::create_dir(&path).unwrap();
    let a = entry("alpha", &template_text("a"));
    assert_eq!(
        err_text(plan(&dir, &[&a], &Manifest::default())),
        format!("I/O error: {} is not a regular file", path.display())
    );
}

#[test]
fn plan_rejects_an_invalid_entry_name() {
    let d = Dest::new();
    let good = entry("alpha", &template_text("a"));
    let mut bad = entry("beta", &template_text("b"));
    bad.name = "../x".to_string();
    match plan(&d.dir(), &[&good, &bad], &Manifest::default()) {
        Err(Error::Usage(m)) => assert_eq!(
            m,
            "invalid template name \"../x\" (use lowercase letters, digits, '-' and '_', starting with a letter or digit)"
        ),
        other => panic!("expected usage error, got {other:?}"),
    }
}

#[test]
fn plan_template_over_the_local_cap_is_an_error() {
    let d = Dest::new();
    let path = d.write("alpha.md", &"a".repeat(LOCAL_CAP as usize + 1));
    let a = entry("alpha", &template_text("a"));
    assert_eq!(
        err_text(plan(&d.dir(), &[&a], &Manifest::default())),
        format!("I/O error: {} is larger than 1048576 bytes", path.display())
    );
}

#[test]
fn plan_template_of_exactly_the_local_cap_is_read() {
    let d = Dest::new();
    d.write("alpha.md", &"a".repeat(LOCAL_CAP as usize));
    let a = entry("alpha", &template_text("a"));
    let items = plan(&d.dir(), &[&a], &Manifest::default()).unwrap();
    assert_eq!(items[0].state, State::Modified);
}
