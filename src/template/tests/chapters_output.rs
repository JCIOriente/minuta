use super::*;

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
