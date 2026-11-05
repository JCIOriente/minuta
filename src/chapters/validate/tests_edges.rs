//! Tests for valid answers, rule boundaries, normalization and inputs that must not panic.

use super::parse_answer;
use super::test_support::{
    answer, chapter, messages, no_limits, parse, parse_with, per_second, segments, NOT_A_TIME,
};
use crate::chapters::ChaptersSpec;

#[test]
fn accepts_valid_answer() {
    let a = answer(&[
        ("0:00", "Intro"),
        ("04:12", "Ownership"),
        ("0:09:00", "Cierre"),
    ]);
    assert_eq!(
        parse(&a).unwrap(),
        vec![
            chapter(0, "Intro"),
            chapter(252, "Ownership"),
            chapter(540, "Cierre")
        ]
    );
}

#[test]
fn accepts_object_inside_prose() {
    let a = format!(
        "Here: {} done",
        answer(&[("0:00", "A"), ("5:00", "B"), ("9:00", "C")])
    );
    assert_eq!(
        parse(&a).unwrap(),
        vec![chapter(0, "A"), chapter(300, "B"), chapter(540, "C")]
    );
}

#[test]
fn min_chapters_boundary() {
    let three = answer(&[("0:00", "A"), ("5:00", "B"), ("9:00", "C")]);
    assert_eq!(
        parse(&three).unwrap(),
        vec![chapter(0, "A"), chapter(300, "B"), chapter(540, "C")]
    );
    let two = answer(&[("0:00", "A"), ("5:00", "B")]);
    assert_eq!(
        messages(parse(&two)),
        vec!["there are 2 chapters; YouTube needs at least 3."]
    );
}

#[test]
fn min_seconds_boundary() {
    let ten = answer(&[("0:00", "A"), ("0:10", "B"), ("5:00", "C")]);
    assert_eq!(
        parse(&ten).unwrap(),
        vec![chapter(0, "A"), chapter(10, "B"), chapter(300, "C")]
    );
    let nine = answer(&[("0:00", "A"), ("0:09", "B"), ("5:00", "C")]);
    assert_eq!(
        messages(parse(&nine)),
        vec![
            r#"chapter 1 ("A") lasts 9 s (0:00 to 0:09); each chapter must last at least 10 s. Merge it with a neighboring chapter or move a start."#
        ]
    );
}

#[test]
fn last_chapter_boundary() {
    let ten = answer(&[("0:00", "A"), ("5:00", "B"), ("9:50", "C")]);
    assert_eq!(
        parse(&ten).unwrap(),
        vec![chapter(0, "A"), chapter(300, "B"), chapter(590, "C")]
    );
    let nine = answer(&[("0:00", "A"), ("5:00", "B"), ("9:51", "C")]);
    assert_eq!(
        messages(parse(&nine)),
        vec![
            r#"chapter 3 ("C") lasts 9 s (9:51 to the end at 10:00); each chapter must last at least 10 s. Merge it with a neighboring chapter or move a start."#
        ]
    );
}

#[test]
fn max_boundary() {
    let spec = ChaptersSpec {
        max: Some(4),
        max_chars: None,
    };
    let four = answer(&[("0:00", "A"), ("2:00", "B"), ("5:00", "C"), ("9:00", "D")]);
    assert_eq!(
        parse_with(&four, spec).unwrap(),
        vec![
            chapter(0, "A"),
            chapter(120, "B"),
            chapter(300, "C"),
            chapter(540, "D")
        ]
    );
    let five = answer(&[
        ("0:00", "A"),
        ("2:00", "B"),
        ("5:00", "C"),
        ("7:00", "D"),
        ("9:00", "E"),
    ]);
    assert_eq!(
        messages(parse_with(&five, spec)),
        vec!["there are 5 chapters; the limit is 4. Merge related chapters to keep at most 4."]
    );
}

#[test]
fn max_chars_boundary_multibyte() {
    // "0:00 é\n5:00 🎉\n9:00 x" = 20 characters (25 bytes).
    let a = answer(&[("0:00", "é"), ("5:00", "🎉"), ("9:00", "x")]);
    let at = ChaptersSpec {
        max: None,
        max_chars: Some(20),
    };
    assert_eq!(
        parse_with(&a, at).unwrap(),
        vec![chapter(0, "é"), chapter(300, "🎉"), chapter(540, "x")]
    );
    let under = ChaptersSpec {
        max: None,
        max_chars: Some(19),
    };
    assert_eq!(
        messages(parse_with(&a, under)),
        vec![
            "the chapters are 20 characters as rendered; the limit is 19. Use fewer chapters or shorter titles."
        ]
    );
}

#[test]
fn max_chars_measured_with_snapped_stamps() {
    // 1:00:05 snaps to 3590: "0:00 A\n30:00 B\n59:50 C" is 22 characters, 24 unsnapped.
    let segs = segments(&[0.0, 1800.0, 3590.0], 3700.0);
    let a = answer(&[("0:00", "A"), ("30:00", "B"), ("1:00:05", "C")]);
    let spec = ChaptersSpec {
        max: None,
        max_chars: Some(22),
    };
    assert_eq!(
        parse_answer(&a, &segs, 3700, &spec).unwrap(),
        vec![chapter(0, "A"), chapter(1800, "B"), chapter(3590, "C")]
    );
}

#[test]
fn same_start_lasts_zero() {
    let segs = segments(&[0.0, 100.0, 300.0], 400.0);
    let a = answer(&[("0:00", "A"), ("1:40", "B"), ("1:50", "C"), ("5:00", "D")]);
    assert_eq!(
        messages(parse_answer(&a, &segs, 400, &no_limits())),
        vec![
            r#"chapter 2 ("B") lasts 0 s (1:40 to 1:40); each chapter must last at least 10 s. Merge it with a neighboring chapter or move a start."#
        ]
    );
}

#[test]
fn title_normalized() {
    let a = answer(&[
        (
            "0:00",
            "  Qué\r\nes\town\u{7}er\u{0}ship   \n  y   más \u{9b} ",
        ),
        ("5:00", "B"),
        ("9:00", "C"),
    ]);
    assert_eq!(
        parse(&a).unwrap(),
        vec![
            chapter(0, "Qué es ownership y más"),
            chapter(300, "B"),
            chapter(540, "C")
        ]
    );
}

#[test]
fn extra_keys_ignored() {
    let input = r#"{"note": 1, "chapters": [
        {"start": "0:00", "title": "A", "end": "5:00"},
        {"start": "5:00", "title": "B", "summary": ["x"]},
        {"start": "9:00", "title": "C"}], "version": 2}"#;
    assert_eq!(
        parse(input).unwrap(),
        vec![chapter(0, "A"), chapter(300, "B"), chapter(540, "C")]
    );
}

#[test]
fn no_panic_array() {
    assert_eq!(
        messages(parse("[1]")),
        vec!["the answer is not a JSON object (found an array instead)"]
    );
}

#[test]
fn no_panic_string() {
    assert_eq!(
        messages(parse(r#""x""#)),
        vec!["the answer is not a JSON object (found a string instead)"]
    );
}

#[test]
fn no_panic_null() {
    assert_eq!(
        messages(parse("null")),
        vec!["the answer is not a JSON object (found null instead)"]
    );
}

#[test]
fn no_panic_number_item() {
    assert_eq!(
        messages(parse(r#"{"chapters": [1]}"#)),
        vec![r#""chapters" must be a list of objects"#]
    );
}

#[test]
fn no_panic_start_only() {
    assert_eq!(
        messages(parse(r#"{"chapters": [{"start": 5}]}"#)),
        vec![
            r#"chapter 1: "start" must be a time like "00:04:12""#,
            r#"chapter 1: missing "title""#,
        ]
    );
}

#[test]
fn no_panic_huge_start() {
    let huge = format!("{}:00", "9".repeat(10_000));
    let a = answer(&[("0:00", "A"), (&huge, "B"), ("9:00", "C")]);
    assert_eq!(messages(parse(&a)), vec![NOT_A_TIME]);
}

#[test]
fn no_panic_huge_numeric_start() {
    // serde_json rejects the number itself, so this is rule 1, not rule 4.
    let huge = "9".repeat(10_000);
    let input = format!(r#"{{"chapters": [{{"start": {huge}, "title": "A"}}]}}"#);
    assert_eq!(
        messages(parse(&input)),
        vec!["the answer is not a JSON object (number out of range at line 1 column 10024)"]
    );
}

#[test]
fn no_panic_empty_segments() {
    let a = answer(&[("0:15", "A"), ("5:00", "B"), ("9:00", "C")]);
    assert_eq!(
        parse_answer(&a, &[], 600, &no_limits()).unwrap(),
        vec![chapter(0, "A"), chapter(300, "B"), chapter(540, "C")]
    );
}

#[test]
fn no_panic_start_after_end() {
    // Only reachable with an `end` below a snapped start; the last chapter lasts 0 s.
    let a = answer(&[("0:00", "A"), ("5:00", "B"), ("9:00", "C")]);
    assert_eq!(
        messages(parse_answer(&a, &per_second(600), 500, &no_limits())),
        vec![
            r#"chapter 3 ("C") lasts 0 s (9:00 to the end at 8:20); each chapter must last at least 10 s. Merge it with a neighboring chapter or move a start."#
        ]
    );
}
