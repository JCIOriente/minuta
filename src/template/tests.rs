use std::path::Path;

use crate::error::Error;
use crate::llm::BackendId;
use crate::template::{Input, Layer, Template, Vars, VARIABLES};

mod chapters_output;
mod fields_output;

fn parse(name: &str, text: &str) -> crate::error::Result<Template> {
    Template::parse(name, Layer::Project, None, text)
}

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
        "probe",
        "join",
        "audio",
        "transcribe",
        "outputs",
        "job",
    ] {
        let m = usage(parse(name, "x"));
        assert_eq!(
            m,
            format!(
                "template {name}: template name '{name}' is reserved (reserved: transcript, probe, join, audio, transcribe, outputs, job)"
            ),
        );
    }
}

#[test]
fn chapters_is_not_reserved() {
    assert!(parse("chapters", "---\noutput: chapters\n---\nx").is_ok());
}

#[test]
fn input_chapters_parses() {
    let t = parse("a", "---\ninput: chapters\n---\nx").unwrap();
    assert_eq!(t.input, Input::Template("chapters".into()));
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
