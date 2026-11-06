use super::*;
use std::os::unix::fs::symlink;

#[test]
fn user_symlink_outside_layer_loads() {
    let e = env();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("t"), "dotfile prompt").unwrap();
    fs::create_dir_all(&e.user).unwrap();
    symlink(outside.path().join("t"), e.user.join("dot.md")).unwrap();
    let c = discover(&e.dirs()).unwrap();
    assert_eq!(c.get("dot").unwrap().body, "dotfile prompt");
}

#[test]
fn relative_symlink_outside_is_invalid() {
    let e = env();
    fs::write(e.project.parent().unwrap().join("x"), "x").unwrap();
    write(&e.project, "real.md", "r");
    symlink("../x", e.project.join("rel.md")).unwrap();
    let c = discover(&e.dirs()).unwrap();
    assert_eq!(c.invalid.len(), 1);
    assert!(
        c.invalid[0].error.contains("outside"),
        "{}",
        c.invalid[0].error
    );
}

#[test]
fn project_dir_symlinked_outside_is_invalid() {
    let tmp = TempDir::new().unwrap();
    let cwd = tmp.path().join("cwd");
    let outside = tmp.path().join("outside");
    write(&outside, "evil.md", "evil");
    fs::create_dir_all(&cwd).unwrap();
    symlink(&outside, cwd.join("templates")).unwrap();
    let dirs = Dirs {
        user: None,
        project: cwd.join("templates"),
    };
    let c = discover(&dirs).unwrap();
    assert!(!c.templates.contains_key("evil"));
    assert_eq!(c.invalid.len(), 1);
    assert!(
        c.invalid[0].error.contains("is a symlink"),
        "{}",
        c.invalid[0].error
    );
}

#[test]
fn templates_symlinked_to_cwd_does_not_leak() {
    let tmp = TempDir::new().unwrap();
    let cwd = tmp.path().join("cwd");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(cwd.join(".env"), "SECRET=1").unwrap();
    symlink(".", cwd.join("templates")).unwrap();
    symlink(".env", cwd.join("summary.md")).unwrap();
    let dirs = Dirs {
        user: None,
        project: cwd.join("templates"),
    };
    let c = discover(&dirs).unwrap();
    let m = usage(c.get("summary"));
    assert!(m.contains("is a symlink"), "{m}");
    assert!(!m.contains("SECRET"), "{m}");
}

#[test]
fn dangling_project_file_shadows_lower_layer() {
    let e = env();
    fs::create_dir_all(&e.project).unwrap();
    symlink(e.project.join("missing"), e.project.join("summary.md")).unwrap();
    let c = discover(&e.dirs()).unwrap();
    let m = usage(c.get("summary"));
    assert!(m.contains("summary.md"), "{m}");
}

#[test]
fn symlink_outside_layer_is_invalid() {
    let e = env();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("secret"), "secret").unwrap();
    fs::create_dir_all(&e.project).unwrap();
    symlink(outside.path().join("secret"), e.project.join("keys.md")).unwrap();
    let c = discover(&e.dirs()).unwrap();
    assert!(!c.templates.contains_key("keys"));
    assert_eq!(c.invalid.len(), 1);
    let err = &c.invalid[0].error;
    assert!(err.contains("keys.md"), "{err}");
    assert!(err.contains("outside the templates directory"), "{err}");
}

#[test]
fn symlink_inside_layer_is_fine() {
    let e = env();
    write(&e.project, "real.md", "prompt");
    symlink(e.project.join("real.md"), e.project.join("alias.md")).unwrap();
    let c = discover(&e.dirs()).unwrap();
    assert_eq!(c.get("alias").unwrap().body, "prompt");
    assert!(c.invalid.is_empty());
}

#[test]
fn dangling_symlink_is_invalid_not_fatal() {
    let e = env();
    fs::create_dir_all(&e.project).unwrap();
    symlink(e.project.join("missing"), e.project.join("gone.md")).unwrap();
    let c = discover(&e.dirs()).unwrap();
    assert_eq!(c.invalid.len(), 1);
    assert!(
        c.invalid[0].error.contains("gone.md"),
        "{}",
        c.invalid[0].error
    );
}
