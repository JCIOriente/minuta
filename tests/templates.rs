use std::fs;
use std::path::{Path, PathBuf};

use minuta::chapters::ChaptersSpec;
use minuta::error::Error;
use minuta::fields::{Field, FieldsSpec, Format, Limit};
use minuta::llm::BackendId;
use minuta::template::discover::{discover, Dirs};
use minuta::template::{Input, Layer, Output, Template, Vars, VARIABLES};
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

const FIELDS_HEAD: &str = "---\noutput: fields\n";

fn fields_tpl(extra: &str) -> String {
    format!("{FIELDS_HEAD}{extra}---\nx")
}

fn fields_error(extra: &str) -> String {
    usage(parse("a", &fields_tpl(extra)))
}

fn spec(t: &Template) -> &FieldsSpec {
    match &t.output {
        Output::Fields(s) => s,
        other => panic!("expected fields output, got {other:?}"),
    }
}

#[test]
fn fields_frontmatter_parses() {
    let t = parse(
        "a",
        &fields_tpl(
            "fields: [tags, title, body]\nlimits:\n  title: { max_chars: 100 }\n  tags: { max: 15, max_chars: 500 }\n",
        ),
    )
    .unwrap();
    let s = spec(&t);
    let got: Vec<(Field, Limit)> = s.fields.iter().map(|f| (f.field, f.limit)).collect();
    assert_eq!(
        got,
        [
            (
                Field::Tags,
                Limit {
                    max_chars: Some(500),
                    max: Some(15)
                }
            ),
            (
                Field::Title,
                Limit {
                    max_chars: Some(100),
                    max: None
                }
            ),
            (Field::Body, Limit::default()),
        ]
    );
}

#[test]
fn format_defaults_to_plain() {
    let t = parse("a", &fields_tpl("fields: [body]\n")).unwrap();
    assert_eq!(spec(&t).format, Format::Plain);
}

#[test]
fn format_markdown() {
    let t = parse("a", &fields_tpl("fields: [body]\nformat: markdown\n")).unwrap();
    assert_eq!(spec(&t).format, Format::Markdown);
}

#[test]
fn format_plain_explicit() {
    let t = parse("a", &fields_tpl("fields: [body]\nformat: plain\n")).unwrap();
    assert_eq!(spec(&t).format, Format::Plain);
}

#[test]
fn markdown_template_output_markdown() {
    let text = "---\noutput: markdown\n---\nBody {{ job_name }}";
    let t = parse("a", text).unwrap();
    assert_eq!(t.output, Output::Markdown);
    assert_eq!(t.raw, text);
    assert_eq!(t.body, "Body {{ job_name }}");
    assert_eq!(parse("b", "plain").unwrap().output, Output::Markdown);
}

fn assert_names_template(m: &str) {
    assert!(m.starts_with("template a: "), "{m}");
}

#[test]
fn fields_error_fields_without_output() {
    let m = usage(parse("a", "---\nfields: [body]\n---\nx"));
    assert_names_template(&m);
    assert!(m.contains("'fields' requires 'output: fields'"), "{m}");
}

#[test]
fn fields_error_limits_without_output() {
    let m = usage(parse("a", "---\nlimits:\n  body: { max_chars: 5 }\n---\nx"));
    assert_names_template(&m);
    assert!(m.contains("'limits' requires 'output: fields'"), "{m}");
}

#[test]
fn fields_error_format_without_output() {
    let m = usage(parse("a", "---\nformat: plain\n---\nx"));
    assert_names_template(&m);
    assert!(m.contains("'format' requires 'output: fields'"), "{m}");
}

#[test]
fn fields_error_missing_fields() {
    let m = fields_error("");
    assert_names_template(&m);
    assert!(m.contains("'output: fields' requires 'fields'"), "{m}");
}

#[test]
fn fields_error_empty_list() {
    let m = fields_error("fields: []\n");
    assert_names_template(&m);
    assert!(m.contains("'fields' must not be empty"), "{m}");
}

#[test]
fn fields_error_unknown_field() {
    let m = fields_error("fields: [headline]\n");
    assert_names_template(&m);
    assert_eq!(
        m,
        "template a: unknown field \"headline\" (known: title, body, tags, hashtags, thumbnail_text, notes, chapters)"
    );
}

#[test]
fn fields_chapters_parses() {
    let t = parse("a", &fields_tpl("fields: [body, chapters]\n")).unwrap();
    let got: Vec<(Field, Limit)> = spec(&t).fields.iter().map(|f| (f.field, f.limit)).collect();
    assert_eq!(
        got,
        [
            (Field::Body, Limit::default()),
            (Field::Chapters, Limit::default())
        ]
    );
}

#[test]
fn fields_error_only_chapters() {
    let m = fields_error("fields: [chapters]\n");
    assert_eq!(
        m,
        "template a: 'fields' needs at least one field besides 'chapters'"
    );
}

#[test]
fn fields_error_chapters_limit() {
    let m = fields_error("fields: [body, chapters]\nlimits:\n  chapters: { max: 3 }\n");
    assert_eq!(
        m,
        "template a: the chapters field is limited in the chapters template"
    );
}

#[test]
fn fields_error_duplicate() {
    let m = fields_error("fields: [body, tags, body]\n");
    assert_names_template(&m);
    assert!(m.contains("duplicate field \"body\""), "{m}");
}

#[test]
fn fields_error_limit_for_undeclared() {
    let m = fields_error("fields: [body]\nlimits:\n  title: { max_chars: 5 }\n");
    assert_names_template(&m);
    assert!(
        m.contains("limit for \"title\", which is not in 'fields'"),
        "{m}"
    );
}

#[test]
fn fields_error_duplicate_limit() {
    let m = fields_error(
        "fields: [title]\nlimits:\n  title: { max_chars: 10 }\n  title: { max_chars: 100 }\n",
    );
    assert_names_template(&m);
    assert!(m.contains("duplicate limit for \"title\""), "{m}");
}

#[test]
fn fields_error_unknown_limit_key() {
    let m = fields_error("fields: [body]\nlimits:\n  body: { max_words: 5 }\n");
    assert_names_template(&m);
    assert!(m.contains("max_words"), "{m}");
}

#[test]
fn fields_error_max_on_text() {
    let m = fields_error("fields: [body]\nlimits:\n  body: { max: 3 }\n");
    assert_names_template(&m);
    assert!(
        m.contains("\"body\" is a text field; 'max' applies to lists only"),
        "{m}"
    );
}

#[test]
fn fields_error_max_chars_zero() {
    let m = fields_error("fields: [body]\nlimits:\n  body: { max_chars: 0 }\n");
    assert_names_template(&m);
    assert!(
        m.contains("'max_chars' of \"body\" must be at least 1"),
        "{m}"
    );
}

#[test]
fn fields_error_max_zero() {
    let m = fields_error("fields: [tags]\nlimits:\n  tags: { max: 0 }\n");
    assert_names_template(&m);
    assert!(m.contains("'max' of \"tags\" must be at least 1"), "{m}");
}

#[test]
fn fields_error_unknown_format() {
    let m = fields_error("fields: [body]\nformat: html\n");
    assert_names_template(&m);
    assert!(
        m.contains("unknown format 'html' (use plain or markdown)"),
        "{m}"
    );
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

const CHAPTERS_HEAD: &str = "---\noutput: chapters\n";

fn chapters_error(name: &str, extra: &str) -> String {
    usage(parse(name, &format!("{CHAPTERS_HEAD}{extra}---\nx")))
}

#[test]
fn chapters_output_parses() {
    let t = parse(
        "chapters",
        "---\noutput: chapters\nlimits:\n  chapters: { max: 20, max_chars: 1500 }\n---\nx",
    )
    .unwrap();
    assert_eq!(
        t.output,
        Output::Chapters(ChaptersSpec {
            max: Some(20),
            max_chars: Some(1500)
        })
    );
}

#[test]
fn chapters_output_default_limits() {
    let t = parse("chapters", "---\noutput: chapters\n---\nx").unwrap();
    assert_eq!(
        t.output,
        Output::Chapters(ChaptersSpec {
            max: None,
            max_chars: None
        })
    );
}

#[test]
fn chapters_error_other_name() {
    assert_eq!(
        chapters_error("a", ""),
        "template a: 'output: chapters' is only for the template named 'chapters'"
    );
}

#[test]
fn chapters_error_missing_output_markdown() {
    assert_eq!(
        usage(parse("chapters", "---\noutput: markdown\n---\nx")),
        "template chapters: the template named 'chapters' requires 'output: chapters'"
    );
}

#[test]
fn chapters_error_missing_output_no_frontmatter() {
    assert_eq!(
        usage(parse("chapters", "x")),
        "template chapters: the template named 'chapters' requires 'output: chapters'"
    );
}

#[test]
fn chapters_error_missing_output_fields() {
    assert_eq!(
        usage(parse(
            "chapters",
            "---\noutput: fields\nfields: [body]\n---\nx"
        )),
        "template chapters: the template named 'chapters' requires 'output: chapters'"
    );
}

#[test]
fn chapters_error_input() {
    assert_eq!(
        chapters_error("chapters", "input: summary\n"),
        "template chapters: 'output: chapters' reads the transcript, not 'summary'"
    );
}

#[test]
fn chapters_error_fields() {
    assert_eq!(
        chapters_error("chapters", "fields: [body]\n"),
        "template chapters: 'fields' is not allowed with 'output: chapters'"
    );
}

#[test]
fn chapters_error_format() {
    assert_eq!(
        chapters_error("chapters", "format: plain\n"),
        "template chapters: 'format' is not allowed with 'output: chapters'"
    );
}

#[test]
fn chapters_error_other_limit_key() {
    assert_eq!(
        chapters_error("chapters", "limits:\n  title: { max_chars: 5 }\n"),
        "template chapters: limit for \"title\"; a chapters template only limits 'chapters'"
    );
}

#[test]
fn chapters_error_unknown_limit_key() {
    assert_eq!(
        chapters_error("chapters", "limits:\n  chapters: { max_words: 5 }\n"),
        "template chapters: built-in: invalid frontmatter: limits.chapters: unknown field `max_words`, expected `max_chars` or `max` at line 4 column 15"
    );
}

#[test]
fn chapters_error_max_below_three() {
    assert_eq!(
        chapters_error("chapters", "limits:\n  chapters: { max: 2 }\n"),
        "template chapters: 'max' of \"chapters\" must be at least 3 (YouTube needs 3 chapters)"
    );
}

#[test]
fn chapters_error_max_chars_zero() {
    assert_eq!(
        chapters_error("chapters", "limits:\n  chapters: { max_chars: 0 }\n"),
        "template chapters: 'max_chars' of \"chapters\" must be at least 1"
    );
}

#[test]
fn chapters_limits_at_minimum_parse() {
    let t = parse(
        "chapters",
        "---\noutput: chapters\nlimits:\n  chapters: { max: 3, max_chars: 1 }\n---\nx",
    )
    .unwrap();
    assert_eq!(
        t.output,
        Output::Chapters(ChaptersSpec {
            max: Some(3),
            max_chars: Some(1)
        })
    );
}

#[test]
fn chapters_error_duplicate_limit() {
    assert_eq!(
        chapters_error(
            "chapters",
            "limits:\n  chapters: { max: 5 }\n  chapters: { max: 5 }\n"
        ),
        "template chapters: duplicate limit for \"chapters\""
    );
}

#[test]
fn chapters_explicit_input_transcript_parses() {
    let t = parse(
        "chapters",
        "---\noutput: chapters\ninput: transcript\n---\nx",
    )
    .unwrap();
    assert_eq!(t.input, Input::Transcript);
    assert_eq!(t.output, Output::Chapters(ChaptersSpec::default()));
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

fn builtin(name: &str) -> Template {
    let e = env();
    let c = discover(&Dirs {
        user: None,
        project: e.project.clone(),
    })
    .unwrap();
    c.get(name).unwrap().clone()
}

fn builtin_limit(name: &str, field: Field) -> Limit {
    let t = builtin(name);
    spec(&t)
        .fields
        .iter()
        .find(|f| f.field == field)
        .unwrap()
        .limit
}

fn builtin_shape(name: &str, fields: &[Field]) -> String {
    let t = builtin(name);
    assert_eq!(t.layer, Layer::BuiltIn);
    assert_eq!(t.input, Input::Transcript);
    let s = spec(&t);
    assert_eq!(s.format, Format::Plain);
    let got: Vec<Field> = s.fields.iter().map(|f| f.field).collect();
    assert_eq!(got, fields);
    assert!(t.description.is_some());
    let out = t.render(&vars());
    assert!(!out.contains("{{"), "{out}");
    assert!(out.contains("reunion"), "{out}");
    assert!(out.contains("Do not invent"), "{out}");
    out
}

#[test]
fn builtin_youtube_shape() {
    let out = builtin_shape(
        "youtube",
        &[
            Field::Title,
            Field::Body,
            Field::Chapters,
            Field::Hashtags,
            Field::Tags,
            Field::ThumbnailText,
        ],
    );
    assert!(out.contains("searchable"), "{out}");
    assert!(out.contains("first two lines"), "{out}");
}

#[test]
fn builtin_youtube_chapters_instruction() {
    let out = builtin("youtube").render(&vars());
    let lines: Vec<&str> = out.lines().filter(|l| l.contains("chapters")).collect();
    assert_eq!(
        lines,
        ["- Do not write chapters or timestamps in the body. The chapter list is added after it."]
    );
}

#[test]
fn builtin_youtube_declares_hashtags_after_chapters() {
    assert_eq!(
        builtin_limit("youtube", Field::Hashtags),
        Limit {
            max_chars: Some(80),
            max: Some(5),
        }
    );
    let out = builtin("youtube").render(&vars());
    let hashtags = out.find("- hashtags:").expect("a hashtags instruction");
    assert!(out.find("- body:").unwrap() < hashtags, "{out}");
    assert!(hashtags < out.find("- tags:").unwrap(), "{out}");
}

#[test]
fn builtin_youtube_no_placeholder() {
    let out = builtin("youtube").render(&vars());
    assert_eq!(out.matches("[Add chapters").count(), 0, "{out}");
}

#[test]
fn builtin_linkedin_shape() {
    let out = builtin_shape("linkedin", &[Field::Body, Field::Hashtags]);
    assert!(out.contains("is a hook"), "{out}");
    assert!(out.contains("short paragraphs"), "{out}");
    assert!(out.contains("Do not include links"), "{out}");
}

#[test]
fn builtin_instagram_shape() {
    let out = builtin_shape("instagram", &[Field::Body, Field::Hashtags]);
    assert!(out.contains("Open with a hook"), "{out}");
    assert!(out.contains("optional call to action"), "{out}");
}

#[test]
fn builtin_chapters_shape() {
    let t = builtin("chapters");
    assert_eq!(t.layer, Layer::BuiltIn);
    assert_eq!(t.input, Input::Transcript);
    assert_eq!(
        t.output,
        Output::Chapters(ChaptersSpec {
            max: Some(20),
            max_chars: Some(1500)
        })
    );
    assert_eq!(
        t.description.as_deref(),
        Some("Timestamped chapters (YouTube rules)")
    );
}

#[test]
fn builtin_chapters_max_limit() {
    let Output::Chapters(spec) = builtin("chapters").output else {
        panic!("chapters is not a chapters output");
    };
    assert_eq!(spec.max, Some(20));
}

#[test]
fn builtin_chapters_max_chars_limit() {
    let Output::Chapters(spec) = builtin("chapters").output else {
        panic!("chapters is not a chapters output");
    };
    assert_eq!(spec.max_chars, Some(1500));
}

#[test]
fn builtin_chapters_prompt_exact() {
    assert_eq!(
        builtin("chapters").render(&vars()),
        "Divide the recording \"reunion\" (1:02:03) into chapters by topic, for the YouTube\n\
         description. Start a chapter where the speakers move to a new topic, not at every pause. Each\n\
         title names the topic in a few words that a viewer would recognize. Use only topics that are\n\
         in the input.\n"
    );
}

fn chars(n: usize) -> Limit {
    Limit {
        max_chars: Some(n),
        max: None,
    }
}

#[test]
fn builtin_youtube_title_limit() {
    assert_eq!(builtin_limit("youtube", Field::Title), chars(100));
}

#[test]
fn builtin_youtube_body_limit() {
    assert_eq!(builtin_limit("youtube", Field::Body), chars(3200));
}

#[test]
fn builtin_youtube_tags_max_limit() {
    assert_eq!(builtin_limit("youtube", Field::Tags).max, Some(15));
}

#[test]
fn builtin_youtube_tags_max_chars_limit() {
    assert_eq!(builtin_limit("youtube", Field::Tags).max_chars, Some(500));
}

#[test]
fn builtin_youtube_thumbnail_text_limit() {
    assert_eq!(builtin_limit("youtube", Field::ThumbnailText), chars(40));
}

#[test]
fn builtin_linkedin_body_limit() {
    assert_eq!(builtin_limit("linkedin", Field::Body), chars(2800));
}

#[test]
fn builtin_linkedin_hashtags_max_limit() {
    assert_eq!(builtin_limit("linkedin", Field::Hashtags).max, Some(5));
}

#[test]
fn builtin_linkedin_hashtags_max_chars_limit() {
    assert_eq!(
        builtin_limit("linkedin", Field::Hashtags).max_chars,
        Some(150)
    );
}

#[test]
fn builtin_instagram_body_limit() {
    assert_eq!(builtin_limit("instagram", Field::Body), chars(1800));
}

#[test]
fn builtin_instagram_hashtags_max_limit() {
    assert_eq!(builtin_limit("instagram", Field::Hashtags).max, Some(30));
}

#[test]
fn builtin_instagram_hashtags_max_chars_limit() {
    assert_eq!(
        builtin_limit("instagram", Field::Hashtags).max_chars,
        Some(390)
    );
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

#[cfg(unix)]
mod hard_links {
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

#[test]
fn social_builtins_read_transcript() {
    for name in ["linkedin", "instagram", "youtube"] {
        let t = builtin(name);
        assert_eq!(t.input, Input::Transcript, "{name}");
        assert!(!t.body.contains("summary"), "{name}: {}", t.body);
    }
}

#[test]
fn social_run_creates_no_summary() {
    let e = env();
    let c = discover(&Dirs {
        user: None,
        project: e.project.clone(),
    })
    .unwrap();
    let deps_of = |name: &str| {
        c.get(name).ok().map(|t| match &t.input {
            Input::Transcript => Vec::new(),
            Input::Template(dep) => vec![dep.clone()],
        })
    };
    let requested: Vec<String> = ["linkedin", "instagram", "youtube"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let order = minuta::template::graph::order(&requested, &deps_of).unwrap();
    assert_eq!(order, requested);
    let order = minuta::template::graph::order(&["summary".to_string()], &deps_of).unwrap();
    assert_eq!(order, ["summary"]);
}
