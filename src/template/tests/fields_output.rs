use super::*;
use crate::fields::{Field, FieldsSpec, Format, Limit};
use crate::template::Output;

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
