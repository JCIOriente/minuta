use std::fs;
use std::path::{Path, PathBuf};

use minuta::error::Error;
use minuta::llm::BackendId;
use minuta::template::discover::{discover, Dirs};
use minuta::template::{Input, Layer, Template, Vars, VARIABLES};
use tempfile::TempDir;

fn parse(name: &str, text: &str) -> minuta::error::Result<Template> {
    Template::parse(name, Layer::Project, None, text)
}

fn usage(r: minuta::error::Result<impl std::fmt::Debug>) -> String {
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

#[test]
fn frontmatter_defaults() {
    let t = parse("a", "---\n---\nbody\n").unwrap();
    assert_eq!(t.input, Input::Transcript);
    assert_eq!(t.description, None);
    assert_eq!(t.llm, None);
    assert_eq!(t.lang, None);
    assert_eq!(t.body, "body\n");
}

#[test]
fn frontmatter_each_key() {
    let t = parse("a", "---\ndescription: Short\n---\nx").unwrap();
    assert_eq!(t.description.as_deref(), Some("Short"));
    let t = parse("a", "---\ninput: b\n---\nx").unwrap();
    assert_eq!(t.input, Input::Template("b".into()));
    let t = parse("a", "---\ninput: transcript\n---\nx").unwrap();
    assert_eq!(t.input, Input::Transcript);
    assert!(parse("a", "---\noutput: markdown\n---\nx").is_ok());
    let t = parse("a", "---\nllm: claude:sonnet\n---\nx").unwrap();
    assert_eq!(
        t.llm,
        Some(BackendId::Claude {
            model: Some("sonnet".into())
        })
    );
    let t = parse("a", "---\nlang: es\n---\nx").unwrap();
    assert_eq!(t.lang.as_deref(), Some("es"));
}

#[test]
fn frontmatter_errors_name_the_template() {
    for text in [
        "---\nllm: gpt\n---\nx",
        "---\nlang: xx\n---\nx",
        "---\ninput: Bad Name\n---\nx",
        "---\noutput: html\n---\nx",
        "---\ndescription: x\nbody\n",
    ] {
        let m = usage(parse("mine", text));
        assert!(m.contains("template mine"), "{text:?}: {m}");
    }
}

#[test]
fn raw_is_the_full_file_text() {
    let text = "---\ndescription: d\n---\nbody\n";
    assert_eq!(parse("a", text).unwrap().raw, text);
}

#[test]
fn bom_and_fence_whitespace_accepted() {
    let t = parse("a", "\u{feff}---\ndescription: d\n---\nbody").unwrap();
    assert_eq!(t.description.as_deref(), Some("d"));
    let t = parse("a", "---  \r\ndescription: d\r\n---\t\r\nbody").unwrap();
    assert_eq!(t.description.as_deref(), Some("d"));
    assert_eq!(t.body, "body");
}

#[test]
fn unterminated_braces_kept_literal() {
    let t = parse("a", "x {{ y").unwrap();
    assert_eq!(t.render(&vars()), "x {{ y");
}

#[test]
fn indented_fence_in_block_scalar_does_not_close() {
    let t = parse("a", "---\ndescription: |\n  x\n  ---\n  y\n---\nbody").unwrap();
    assert_eq!(t.description.as_deref(), Some("x\n---\ny\n"));
    assert_eq!(t.body, "body");
}

#[test]
fn bom_without_frontmatter_not_in_body() {
    let t = parse("a", "\u{feff}just body").unwrap();
    assert_eq!(t.body, "just body");
}

#[test]
fn no_frontmatter_is_body() {
    let t = parse("a", "Just a prompt.\n---\nnot a fence\n").unwrap();
    assert_eq!(t.body, "Just a prompt.\n---\nnot a fence\n");
    assert_eq!(t.input, Input::Transcript);
    assert_eq!(t.raw, t.body);
}

#[test]
fn fields_output_rejected() {
    let m = usage(parse("a", "---\noutput: fields\n---\nx"));
    assert!(m.contains("later version"), "{m}");
    assert!(m.contains("template a"), "{m}");
}

#[test]
fn unknown_key_rejected() {
    let m = usage(parse("a", "---\nlimits: 3\n---\nx"));
    assert!(m.contains("template a"), "{m}");
    assert!(m.contains("limits"), "{m}");
}

#[test]
fn yaml_error_has_line() {
    let path = Path::new("/t/a.md").to_path_buf();
    let r = Template::parse(
        "a",
        Layer::User,
        Some(path),
        "---\ndescription: ok\nllm: [unclosed\n---\nx",
    );
    let m = usage(r);
    assert!(m.contains("/t/a.md"), "{m}");
    assert!(m.contains("line 3"), "{m}");
}

#[test]
fn invalid_names_rejected() {
    for name in ["", "A", "-a", "_a", "a b", "a.b", "é", "a/b"] {
        let m = usage(parse(name, "x"));
        assert!(m.contains("invalid template name"), "{name:?}: {m}");
    }
}

#[test]
fn valid_names_accepted() {
    for name in ["a", "0a", "a-b_c", "minutes2"] {
        assert!(parse(name, "x").is_ok(), "{name}");
    }
}

#[test]
fn reserved_names_rejected() {
    for name in [
        "transcript",
        "chapters",
        "probe",
        "join",
        "audio",
        "transcribe",
        "outputs",
        "job",
    ] {
        let m = usage(parse(name, "x"));
        assert!(m.contains("reserved"), "{name}: {m}");
    }
}

#[test]
fn renders_each_variable() {
    let v = vars();
    for (var, expected) in [
        ("output_lang", "Spanish"),
        ("output_lang_code", "es"),
        ("speech_lang", "Catalan"),
        ("job_name", "reunion"),
        ("duration", "1:02:03"),
    ] {
        let t = parse("a", &format!("[{{{{{var}}}}}]")).unwrap();
        assert_eq!(t.render(&v), format!("[{expected}]"), "{var}");
    }
}

#[test]
fn variables_allow_spaces_and_repeat() {
    let t = parse("a", "{{ job_name }} / {{job_name}} / {{  duration  }}").unwrap();
    assert_eq!(t.render(&vars()), "reunion / reunion / 1:02:03");
}

#[test]
fn unknown_variable_rejected() {
    let m = usage(parse("a", "hello {{ nope }}"));
    assert!(m.contains("nope"), "{m}");
    for v in VARIABLES {
        assert!(m.contains(v), "{m}");
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
    assert_eq!(c.templates.keys().collect::<Vec<_>>(), ["summary"]);
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
    assert_eq!(c.templates.len(), 1);
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
    assert!(m.contains("minutes, summary"), "{m}");
}

#[test]
fn builtin_summary_parses() {
    let e = env();
    let c = discover(&Dirs {
        user: None,
        project: e.project.clone(),
    })
    .unwrap();
    let t = c.get("summary").unwrap();
    assert_eq!(t.input, Input::Transcript);
    let out = t.render(&vars());
    assert!(out.contains("reunion"), "{out}");
    assert!(out.contains("1:02:03"), "{out}");
    for word in ["overview", "Key points", "Decisions", "Action items"] {
        assert!(out.contains(word), "{word}");
    }
    assert!(!out.contains("{{"), "{out}");
}

#[cfg(unix)]
mod symlinks {
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
        fs::write(e.project.parent().unwrap().join("x"), "x").ok();
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
}
