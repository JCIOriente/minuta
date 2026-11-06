use super::*;
use std::os::unix::fs::symlink;

fn hard_linked_message(path: &Path) -> String {
    format!(
        "{}: file is hard-linked; hard links are not loaded from the project templates directory",
        path.display()
    )
}

fn outside_file() -> (TempDir, PathBuf) {
    let outside = TempDir::new().unwrap();
    let file = outside.path().join("secret");
    fs::write(&file, "secret prompt").unwrap();
    (outside, file)
}

#[test]
fn project_template_hard_link_rejected() {
    let e = env();
    let (_outside, secret) = outside_file();
    fs::create_dir_all(&e.project).unwrap();
    let link = e.project.join("x.md");
    fs::hard_link(&secret, &link).unwrap();
    let c = discover(&e.dirs()).unwrap();
    assert!(!c.templates.contains_key("x"));
    assert_eq!(c.invalid.len(), 1);
    assert_eq!(c.invalid[0].path, link);
    assert_eq!(c.invalid[0].error, hard_linked_message(&link));
}

#[test]
fn project_hard_link_shadows_lower_layer() {
    let e = env();
    let (_outside, secret) = outside_file();
    write(&e.user, "summary.md", "user prompt");
    fs::create_dir_all(&e.project).unwrap();
    let link = e.project.join("summary.md");
    fs::hard_link(&secret, &link).unwrap();
    let c = discover(&e.dirs()).unwrap();
    assert_eq!(usage(c.get("summary")), hard_linked_message(&link));
}

#[test]
fn project_symlink_to_hard_linked_file_rejected() {
    let e = env();
    let (_outside, secret) = outside_file();
    fs::create_dir_all(&e.project).unwrap();
    let real = e.project.join("real.md");
    let alias = e.project.join("alias.md");
    fs::hard_link(&secret, &real).unwrap();
    symlink(&real, &alias).unwrap();
    let c = discover(&e.dirs()).unwrap();
    let mut errors: Vec<String> = c.invalid.iter().map(|i| i.error.clone()).collect();
    errors.sort_unstable();
    let mut expected = vec![hard_linked_message(&alias), hard_linked_message(&real)];
    expected.sort_unstable();
    assert_eq!(errors, expected);
    assert!(!c.templates.contains_key("alias"));
    assert!(!c.templates.contains_key("real"));
}

#[test]
fn user_layer_hard_link_allowed() {
    let e = env();
    let (_outside, secret) = outside_file();
    fs::create_dir_all(&e.user).unwrap();
    fs::hard_link(&secret, e.user.join("dot.md")).unwrap();
    let c = discover(&e.dirs()).unwrap();
    assert_eq!(c.get("dot").unwrap().body, "secret prompt");
    assert!(c.invalid.is_empty());
}
