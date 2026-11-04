use serde_json::json;

use super::parse_answer;
use super::test_support::{answer, messages, no_limits, parse, parse_with, per_second, NOT_A_TIME};
use crate::chapters::ChaptersSpec;

#[test]
fn violation_rule_1_not_an_object() {
    assert_eq!(
        messages(parse("I cannot do that")),
        vec!["the answer is not a JSON object (expected value at line 1 column 1)"]
    );
}

#[test]
fn violation_rule_2_missing_chapters() {
    assert_eq!(
        messages(parse(r#"{"chapter": []}"#)),
        vec![r#"missing key "chapters""#]
    );
}

#[test]
fn violation_rule_3_not_a_list() {
    for input in [
        r#"{"chapters": "0:00 Intro"}"#,
        r#"{"chapters": null}"#,
        r#"{"chapters": {"start": "0:00", "title": "A"}}"#,
    ] {
        assert_eq!(
            messages(parse(input)),
            vec![r#""chapters" must be a list of objects"#],
            "{input}"
        );
    }
}

#[test]
fn violation_rule_3_item_not_an_object() {
    let input = r#"{"chapters": [{"start": "0:00", "title": "A"}, "5:00 B", null]}"#;
    assert_eq!(
        messages(parse(input)),
        vec![r#""chapters" must be a list of objects"#]
    );
}

#[test]
fn violation_rule_4_missing_start() {
    let input = r#"{"chapters": [
        {"start": "0:00", "title": "A"}, {"title": "B"}, {"start": "9:00", "title": "C"}]}"#;
    assert_eq!(
        messages(parse(input)),
        vec![r#"chapter 2: missing "start""#]
    );
}

#[test]
fn violation_rule_4_start_not_a_time() {
    for start in [
        json!("4m12"),
        json!("1:2"),
        json!("00:60"),
        json!("1:60:00"),
        json!(" 5:00"),
        json!("252"),
        json!(252),
        json!(null),
        json!(["5:00"]),
    ] {
        let input = json!({"chapters": [
            {"start": "0:00", "title": "A"},
            {"start": start, "title": "B"},
            {"start": "9:00", "title": "C"},
        ]})
        .to_string();
        assert_eq!(messages(parse(&input)), vec![NOT_A_TIME], "{start}");
    }
}

#[test]
fn violation_rule_4_missing_title() {
    let input = r#"{"chapters": [
        {"start": "0:00", "title": "A"}, {"start": "5:00"}, {"start": "9:00", "title": "C"}]}"#;
    assert_eq!(
        messages(parse(input)),
        vec![r#"chapter 2: missing "title""#]
    );
}

#[test]
fn violation_rule_4_title_not_a_string() {
    for title in [json!(5), json!(null), json!(["B"]), json!({"text": "B"})] {
        let input = json!({"chapters": [
            {"start": "0:00", "title": "A"},
            {"start": "5:00", "title": title},
            {"start": "9:00", "title": "C"},
        ]})
        .to_string();
        assert_eq!(
            messages(parse(&input)),
            vec![r#"chapter 2: "title" must be a string"#],
            "{title}"
        );
    }
}

#[test]
fn violation_rule_4_title_empty() {
    for title in ["", "  \n\t ", "\u{1}", "\r\n"] {
        let a = answer(&[("0:00", "A"), ("5:00", title), ("9:00", "C")]);
        assert_eq!(
            messages(parse(&a)),
            vec![r#"chapter 2: "title" is empty"#],
            "{title:?}"
        );
    }
}

#[test]
fn violation_rule_4_in_order_within_and_across_chapters() {
    let input = r#"{"chapters": [
        {}, {"start": "5:00", "title": "B"}, {"start": 540, "title": ""}]}"#;
    assert_eq!(
        messages(parse(input)),
        vec![
            r#"chapter 1: missing "start""#,
            r#"chapter 1: missing "title""#,
            r#"chapter 3: "start" must be a time like "00:04:12""#,
            r#"chapter 3: "title" is empty"#,
        ]
    );
}

#[test]
fn violation_rule_5_too_few() {
    let a = answer(&[("0:00", "A"), ("5:00", "B")]);
    assert_eq!(
        messages(parse(&a)),
        vec!["there are 2 chapters; YouTube needs at least 3."]
    );
}

#[test]
fn violation_rule_5_empty_list() {
    assert_eq!(
        messages(parse(r#"{"chapters": []}"#)),
        vec!["there are 0 chapters; YouTube needs at least 3."]
    );
}

#[test]
fn violation_rule_6_over_max() {
    let spec = ChaptersSpec {
        max: Some(3),
        max_chars: None,
    };
    let a = answer(&[("0:00", "A"), ("2:00", "B"), ("5:00", "C"), ("9:00", "D")]);
    assert_eq!(
        messages(parse_with(&a, spec)),
        vec!["there are 4 chapters; the limit is 3. Merge related chapters to keep at most 3."]
    );
}

#[test]
fn violation_rule_7_out_of_order() {
    let a = answer(&[
        ("0:00", "Intro"),
        ("5:00", "Ownership"),
        ("4:12", "Borrowing"),
    ]);
    assert_eq!(
        messages(parse(&a)),
        vec![
            r#"chapter 3 ("Borrowing") starts at 4:12, before chapter 2 at 5:00. List the chapters in the order they happen."#
        ]
    );
}

#[test]
fn violation_rule_8_too_short() {
    let a = answer(&[("0:00", "Introducción"), ("0:04", "B"), ("5:00", "C")]);
    assert_eq!(
        messages(parse(&a)),
        vec![
            r#"chapter 1 ("Introducción") lasts 4 s (0:00 to 0:04); each chapter must last at least 10 s. Merge it with a neighboring chapter or move a start."#
        ]
    );
}

#[test]
fn violation_rule_8_last_too_short() {
    let a = answer(&[("0:00", "A"), ("5:00", "B"), ("12:04", "Cierre")]);
    let result = parse_answer(&a, &per_second(730), 730, &no_limits());
    assert_eq!(
        messages(result),
        vec![
            r#"chapter 3 ("Cierre") lasts 6 s (12:04 to the end at 12:10); each chapter must last at least 10 s. Merge it with a neighboring chapter or move a start."#
        ]
    );
}

#[test]
fn violation_rule_7_reports_each() {
    let a = answer(&[
        ("0:00", "A"),
        ("5:00", "B"),
        ("4:12", "C"),
        ("4:20", "D"),
        ("4:00", "E"),
    ]);
    assert_eq!(
        messages(parse(&a)),
        vec![
            r#"chapter 3 ("C") starts at 4:12, before chapter 2 at 5:00. List the chapters in the order they happen."#,
            r#"chapter 5 ("E") starts at 4:00, before chapter 4 at 4:20. List the chapters in the order they happen."#,
        ]
    );
}

#[test]
fn violation_rule_8_reports_each() {
    let a = answer(&[("0:00", "A"), ("0:04", "B"), ("5:00", "C"), ("9:55", "D")]);
    assert_eq!(
        messages(parse(&a)),
        vec![
            r#"chapter 1 ("A") lasts 4 s (0:00 to 0:04); each chapter must last at least 10 s. Merge it with a neighboring chapter or move a start."#,
            r#"chapter 4 ("D") lasts 5 s (9:55 to the end at 10:00); each chapter must last at least 10 s. Merge it with a neighboring chapter or move a start."#,
        ]
    );
}

#[test]
fn violation_rule_9_over_max_chars() {
    let spec = ChaptersSpec {
        max: None,
        max_chars: Some(19),
    };
    // "0:00 A\n5:00 B\n9:00 C" = 20 characters
    let a = answer(&[("0:00", "A"), ("5:00", "B"), ("9:00", "C")]);
    assert_eq!(
        messages(parse_with(&a, spec)),
        vec![
            "the chapters are 20 characters as rendered; the limit is 19. Use fewer chapters or shorter titles."
        ]
    );
}

#[test]
fn timing_rules_in_one_pass_in_spec_order() {
    let spec = ChaptersSpec {
        max: None,
        max_chars: Some(5),
    };
    let a = answer(&[("0:00", "A"), ("0:04", "B")]);
    assert_eq!(
        messages(parse_with(&a, spec)),
        vec![
            "there are 2 chapters; YouTube needs at least 3.",
            r#"chapter 1 ("A") lasts 4 s (0:00 to 0:04); each chapter must last at least 10 s. Merge it with a neighboring chapter or move a start."#,
            "the chapters are 13 characters as rendered; the limit is 5. Use fewer chapters or shorter titles.",
        ]
    );
}

#[test]
fn structural_skips_timing() {
    let spec = ChaptersSpec {
        max: Some(3),
        max_chars: Some(1),
    };
    // Out of order, too short and too long as well, but chapters 2 and 3 have bad titles.
    let input = r#"{"chapters": [
        {"start": "5:00", "title": "A"}, {"start": "4:12"}, {"start": "4:13", "title": ""}]}"#;
    assert_eq!(
        messages(parse_with(input, spec)),
        vec![
            r#"chapter 2: missing "title""#,
            r#"chapter 3: "title" is empty"#
        ]
    );
}

#[test]
fn order_skips_duration() {
    // Chapter 3 is out of order; it would also be too short (4:12 to 4:15).
    let a = answer(&[("0:00", "A"), ("5:00", "B"), ("4:12", "C"), ("4:15", "D")]);
    assert_eq!(
        messages(parse(&a)),
        vec![
            r#"chapter 3 ("C") starts at 4:12, before chapter 2 at 5:00. List the chapters in the order they happen."#
        ]
    );
}
