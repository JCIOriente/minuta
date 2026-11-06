use super::*;

#[cfg(unix)]
#[test]
fn apply_does_not_write_through_a_planted_temp_symlink() {
    let d = Dest::new();
    let dir = d.create();
    let victim = d.outside("victim.txt", "do not touch");
    std::os::unix::fs::symlink(&victim, dir.join("alpha.md.tmp")).unwrap();
    let text = template_text("a");
    let a = entry("alpha", &text);
    let mut f = Fetcher::new(&[("alpha", text.as_bytes())]);
    run(&dir, &[&a], false, &mut f, &mut Manifest::default()).unwrap();
    assert_eq!(fs::read_to_string(&victim).unwrap(), "do not touch");
    assert_eq!(fs::read_to_string(dir.join("alpha.md")).unwrap(), text);
    assert!(!dir.join("alpha.md").is_symlink());
}

#[cfg(unix)]
#[test]
fn apply_does_not_write_the_manifest_through_a_planted_temp_symlink() {
    let d = Dest::new();
    let dir = d.create();
    let victim = d.outside("victim.txt", "do not touch");
    std::os::unix::fs::symlink(&victim, dir.join(".pulled.toml.tmp")).unwrap();
    let text = template_text("a");
    let a = entry("alpha", &text);
    let mut f = Fetcher::new(&[("alpha", text.as_bytes())]);
    let mut manifest = Manifest::default();
    run(&dir, &[&a], false, &mut f, &mut manifest).unwrap();
    assert_eq!(fs::read_to_string(&victim).unwrap(), "do not touch");
    assert_eq!(
        fs::read_to_string(dir.join(".pulled.toml")).unwrap(),
        manifest.to_toml()
    );
}

#[cfg(unix)]
#[test]
fn apply_does_not_write_a_backup_through_a_planted_temp_symlink() {
    let d = Dest::new();
    let dir = d.create();
    let edited = template_text("edited");
    d.write("alpha.md", &edited);
    let victim = d.outside("victim.txt", "do not touch");
    std::os::unix::fs::symlink(&victim, dir.join("alpha.md.bak.tmp")).unwrap();
    let new = template_text("new");
    let a = entry("alpha", &new);
    let mut f = Fetcher::new(&[("alpha", new.as_bytes())]);
    run(&dir, &[&a], true, &mut f, &mut Manifest::default()).unwrap();
    assert_eq!(fs::read_to_string(&victim).unwrap(), "do not touch");
    assert_eq!(
        fs::read_to_string(dir.join("alpha.md.bak")).unwrap(),
        edited
    );
}
