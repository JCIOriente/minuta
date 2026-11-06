use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::fields::FieldsSpec;
use crate::template::discover::{discover, Dirs};
use crate::template::{Layer, Output, Template, Vars};
use tempfile::TempDir;

mod builtin_templates;
#[cfg(unix)]
mod hard_links;
#[cfg(unix)]
mod symlinks;

fn usage(r: crate::error::Result<impl std::fmt::Debug>) -> String {
    match r {
        Err(Error::Usage(m)) => m,
        other => panic!("expected usage error, got {other:?}"),
    }
}

fn vars() -> Vars {
    Vars {
        output_lang: "Spanish".into(),
        output_lang_code: "es".into(),
        speech_lang: "Catalan".into(),
        job_name: "reunion".into(),
        duration: "1:02:03".into(),
    }
}

fn spec(t: &Template) -> &FieldsSpec {
    match &t.output {
        Output::Fields(s) => s,
        other => panic!("expected fields output, got {other:?}"),
    }
}

fn write(dir: &Path, file: &str, text: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(file), text).unwrap();
}

struct Env {
    _tmp: TempDir,
    user: PathBuf,
    project: PathBuf,
}

fn env() -> Env {
    let tmp = TempDir::new().unwrap();
    let user = tmp.path().join("user");
    let project = tmp.path().join("project");
    Env {
        _tmp: tmp,
        user,
        project,
    }
}

impl Env {
    fn dirs(&self) -> Dirs {
        Dirs {
            user: Some(self.user.clone()),
            project: self.project.clone(),
        }
    }
}

#[test]
fn user_overrides_builtin() {
    let e = env();
    write(&e.user, "summary.md", "user prompt");
    let c = discover(&e.dirs()).unwrap();
    let t = c.get("summary").unwrap();
    assert_eq!(t.layer, Layer::User);
    assert_eq!(t.body, "user prompt");
    assert_eq!(t.path.as_deref(), Some(e.user.join("summary.md").as_path()));
}

#[test]
fn project_overrides_user() {
    let e = env();
    write(&e.user, "minutes.md", "user prompt");
    write(&e.project, "minutes.md", "project prompt");
    let c = discover(&e.dirs()).unwrap();
    let t = c.get("minutes").unwrap();
    assert_eq!(t.layer, Layer::Project);
    assert_eq!(t.body, "project prompt");
}

#[test]
fn missing_dir_is_empty() {
    let e = env();
    let c = discover(&e.dirs()).unwrap();
    assert_eq!(
        c.templates.keys().collect::<Vec<_>>(),
        ["chapters", "instagram", "linkedin", "summary", "youtube"]
    );
    assert_eq!(c.get("summary").unwrap().layer, Layer::BuiltIn);
    assert!(c.get("summary").unwrap().path.is_none());
    assert!(c.invalid.is_empty());
}

#[test]
fn unreadable_dir_is_error() {
    let e = env();
    // A regular file where a directory is expected cannot be listed.
    fs::create_dir_all(e.user.parent().unwrap()).unwrap();
    fs::write(&e.user, "x").unwrap();
    let err = discover(&e.dirs()).unwrap_err();
    assert!(matches!(err, Error::Io(_)), "{err:?}");
    assert!(err.to_string().contains(e.user.to_str().unwrap()), "{err}");
}

#[test]
fn non_md_files_and_dirs_are_ignored() {
    let e = env();
    write(&e.project, "notes.txt", "x");
    fs::create_dir_all(e.project.join("sub.md")).unwrap();
    let c = discover(&e.dirs()).unwrap();
    assert_eq!(c.templates.len(), 5);
    assert!(c.invalid.is_empty());
}

#[test]
fn invalid_file_listed_not_fatal() {
    let e = env();
    write(&e.project, "Bad Name.md", "x");
    write(&e.project, "broken.md", "---\nnope: 1\n---\nx");
    write(&e.project, "good.md", "x");
    let c = discover(&e.dirs()).unwrap();
    assert!(c.templates.contains_key("good"));
    let mut files: Vec<_> = c
        .invalid
        .iter()
        .map(|i| i.path.file_name().unwrap().to_str().unwrap().to_string())
        .collect();
    files.sort();
    assert_eq!(files, ["Bad Name.md", "broken.md"]);
    assert!(c.invalid.iter().all(|i| i.layer == Layer::Project));
    let m = usage(c.get("broken"));
    assert!(m.contains("nope"), "{m}");
}

#[test]
fn invalid_override_shadows_lower_layer() {
    let e = env();
    write(&e.project, "summary.md", "{{ nope }}");
    let c = discover(&e.dirs()).unwrap();
    let m = usage(c.get("summary"));
    assert!(m.contains("nope"), "{m}");
}

#[test]
fn non_utf8_file_names_the_path() {
    let e = env();
    fs::create_dir_all(&e.project).unwrap();
    fs::write(e.project.join("bin.md"), [0xff, 0xfe, 0x00]).unwrap();
    let c = discover(&e.dirs()).unwrap();
    assert_eq!(c.invalid.len(), 1);
    assert!(
        c.invalid[0].error.contains("bin.md"),
        "{}",
        c.invalid[0].error
    );
    let m = usage(c.get("bin"));
    assert!(m.contains("not valid UTF-8"), "{m}");
}

#[test]
fn get_unknown_lists_names() {
    let e = env();
    write(&e.project, "minutes.md", "x");
    let c = discover(&e.dirs()).unwrap();
    let m = usage(c.get("nope"));
    assert!(m.contains("nope"), "{m}");
    assert!(
        m.contains("chapters, instagram, linkedin, minutes, summary, youtube"),
        "{m}"
    );
}

#[test]
fn user_templates_dir_is_under_home_minuta() {
    let tmp = TempDir::new().unwrap();
    let dirs = Dirs::for_home(&tmp.path().join("home"), &tmp.path().join("cwd"));
    assert_eq!(dirs.user, Some(tmp.path().join("home/.minuta/templates")));
    assert_eq!(dirs.project, tmp.path().join("cwd/templates"));
}

#[test]
fn old_user_templates_dir_is_ignored() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    let old = home.join(".config/minuta/templates");
    fs::create_dir_all(&old).unwrap();
    fs::write(old.join("legacy.md"), "---\n---\nold body\n").unwrap();
    let c = discover(&Dirs::for_home(&home, &tmp.path().join("cwd"))).unwrap();
    assert!(!c.templates.contains_key("legacy"));
    assert!(c.invalid.is_empty());
}

#[test]
fn user_layer_overridden_by_project_layer() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    let cwd = tmp.path().join("cwd");
    let user = home.join(".minuta/templates");
    let project = cwd.join("templates");
    fs::create_dir_all(&user).unwrap();
    fs::create_dir_all(&project).unwrap();
    fs::write(user.join("x.md"), "---\n---\nuser body\n").unwrap();
    let c = discover(&Dirs::for_home(&home, &cwd)).unwrap();
    assert_eq!(c.get("x").unwrap().layer, Layer::User);
    fs::write(project.join("x.md"), "---\n---\nproject body\n").unwrap();
    let c = discover(&Dirs::for_home(&home, &cwd)).unwrap();
    assert_eq!(c.get("x").unwrap().body, "project body\n");
    assert_eq!(c.get("x").unwrap().layer, Layer::Project);
}
