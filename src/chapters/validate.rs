//! Chapter validation: parsing and checking the model's chapters (ADR-0008).

use serde_json::{Map, Value as Json};

use super::{lines, parse_stamp, stamp, Chapter, ChaptersSpec, MIN_CHAPTERS, MIN_SECONDS};
use crate::fields::validate::{normalize, parse_object};
use crate::fields::Violation;
use crate::transcript::Segment;

/// `answer` is already cleaned by `call::clean`. `end` = last segment end, floored. Snaps, then
/// checks the rules; all violations in one pass, in the spec's order. With no segments there is
/// nothing to snap to: the starts stay as parsed (the first one still becomes 0).
pub fn parse_answer(
    answer: &str,
    segments: &[Segment],
    end: u64,
    spec: &ChaptersSpec,
) -> Result<Vec<Chapter>, Vec<Violation>> {
    let object = parse_object(answer).map_err(|message| {
        vec![Violation(format!(
            "the answer is not a JSON object ({message})"
        ))]
    })?;
    let Some(raw) = object.get("chapters") else {
        return Err(vec![Violation("missing key \"chapters\"".into())]);
    };
    let Some(items) = object_items(raw) else {
        return Err(vec![Violation(
            "\"chapters\" must be a list of objects".into(),
        )]);
    };
    let mut chapters = Vec::new();
    let mut violations = Vec::new();
    for (i, item) in items.into_iter().enumerate() {
        match check_chapter(i + 1, item) {
            Ok(chapter) => chapters.push(chapter),
            Err(found) => violations.extend(found),
        }
    }
    if !violations.is_empty() {
        return Err(violations);
    }
    let chapters = snap(chapters, segments);
    let violations = check_rules(&chapters, end, spec);
    if violations.is_empty() {
        Ok(chapters)
    } else {
        Err(violations)
    }
}

fn object_items(raw: &Json) -> Option<Vec<&Map<String, Json>>> {
    raw.as_array()?.iter().map(Json::as_object).collect()
}

/// Rule 4 for chapter `n` (1-based): `start` problems, then `title` problems.
fn check_chapter(n: usize, item: &Map<String, Json>) -> Result<Chapter, Vec<Violation>> {
    let mut found = Vec::new();
    let start = match item.get("start") {
        None => {
            found.push(format!("chapter {n}: missing \"start\""));
            None
        }
        Some(raw) => {
            let start = raw.as_str().and_then(parse_stamp);
            if start.is_none() {
                found.push(format!(
                    "chapter {n}: \"start\" must be a time like \"00:04:12\""
                ));
            }
            start
        }
    };
    let title = match item.get("title") {
        None => {
            found.push(format!("chapter {n}: missing \"title\""));
            None
        }
        Some(Json::String(raw)) => {
            let title = one_line(raw);
            if title.is_empty() {
                found.push(format!("chapter {n}: \"title\" is empty"));
                None
            } else {
                Some(title)
            }
        }
        Some(_) => {
            found.push(format!("chapter {n}: \"title\" must be a string"));
            None
        }
    };
    match (start, title) {
        (Some(start), Some(title)) => Ok(Chapter { start, title }),
        _ => Err(found.into_iter().map(Violation).collect()),
    }
}

/// The fields normalization, then every whitespace run becomes one space: one chapter per line.
fn one_line(raw: &str) -> String {
    normalize(raw)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Each start to its nearest segment start, then the first start to 0. Never reorders.
fn snap(chapters: Vec<Chapter>, segments: &[Segment]) -> Vec<Chapter> {
    let mut chapters: Vec<Chapter> = chapters
        .into_iter()
        .map(|c| Chapter {
            start: snap_start(c.start, segments),
            title: c.title,
        })
        .collect();
    if let Some(first) = chapters.first_mut() {
        first.start = 0;
    }
    chapters
}

/// The start of the nearest segment (a tie goes to the earlier one), floored; a binary search
/// over the ascending segment starts. `start` itself when there are no segments.
fn snap_start(start: u64, segments: &[Segment]) -> u64 {
    let at = start as f64;
    let after = segments.partition_point(|s| s.start <= at);
    let before = after.checked_sub(1).and_then(|i| segments.get(i));
    let nearest = match (before, segments.get(after)) {
        (Some(before), Some(next)) if next.start - at < at - before.start => next,
        (Some(before), _) => before,
        (None, Some(next)) => next,
        (None, None) => return start,
    };
    nearest.start.floor() as u64
}

/// Rules 5-9 on the snapped chapters, in the spec's order; rule 8 only when rule 7 found nothing.
fn check_rules(chapters: &[Chapter], end: u64, spec: &ChaptersSpec) -> Vec<Violation> {
    let count = chapters.len();
    let mut found = Vec::new();
    if count < MIN_CHAPTERS {
        found.push(format!(
            "there are {count} chapters; YouTube needs at least {MIN_CHAPTERS}."
        ));
    }
    if let Some(max) = spec.max {
        if count > max {
            found.push(format!(
                "there are {count} chapters; the limit is {max}. Merge related chapters to keep at most {max}."
            ));
        }
    }
    let order: Vec<String> = chapters
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[1].start < pair[0].start)
        .map(|(i, pair)| {
            format!(
                "chapter {} (\"{}\") starts at {}, before chapter {} at {}. List the chapters in the order they happen.",
                i + 2,
                pair[1].title,
                stamp(pair[1].start),
                i + 1,
                stamp(pair[0].start)
            )
        })
        .collect();
    if order.is_empty() {
        for (i, chapter) in chapters.iter().enumerate() {
            let (until, to) = match chapters.get(i + 1) {
                Some(next) => (next.start, stamp(next.start)),
                None => (end, format!("the end at {}", stamp(end))),
            };
            let lasts = until.saturating_sub(chapter.start);
            if lasts < MIN_SECONDS {
                found.push(format!(
                    "chapter {} (\"{}\") lasts {lasts} s ({} to {to}); each chapter must last at least {MIN_SECONDS} s. Merge it with a neighboring chapter or move a start.",
                    i + 1,
                    chapter.title,
                    stamp(chapter.start)
                ));
            }
        }
    } else {
        found.extend(order);
    }
    if let Some(max_chars) = spec.max_chars {
        let count = lines(chapters).chars().count();
        if count > max_chars {
            found.push(format!(
                "the chapters are {count} characters as rendered; the limit is {max_chars}. Use fewer chapters or shorter titles."
            ));
        }
    }
    found.into_iter().map(Violation).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn seg(start: f64, end: f64) -> Segment {
        Segment {
            start,
            end,
            text: "x".into(),
        }
    }

    /// Segments starting at `starts`, each ending where the next one starts, the last at `end`.
    fn segments(starts: &[f64], end: f64) -> Vec<Segment> {
        starts
            .iter()
            .enumerate()
            .map(|(i, &start)| seg(start, starts.get(i + 1).copied().unwrap_or(end)))
            .collect()
    }

    /// One-second segments from 0 to `end`, so whole-second starts snap to themselves.
    fn per_second(end: u64) -> Vec<Segment> {
        (0..end).map(|s| seg(s as f64, (s + 1) as f64)).collect()
    }

    fn answer(chapters: &[(&str, &str)]) -> String {
        let items: Vec<_> = chapters
            .iter()
            .map(|(start, title)| json!({"start": start, "title": title}))
            .collect();
        json!({ "chapters": items }).to_string()
    }

    fn chapter(start: u64, title: &str) -> Chapter {
        Chapter {
            start,
            title: title.into(),
        }
    }

    fn no_limits() -> ChaptersSpec {
        ChaptersSpec::default()
    }

    fn messages(result: Result<Vec<Chapter>, Vec<Violation>>) -> Vec<String> {
        result.unwrap_err().into_iter().map(|v| v.0).collect()
    }

    /// One-second segments over 10 minutes, no limits.
    fn parse(answer: &str) -> Result<Vec<Chapter>, Vec<Violation>> {
        parse_answer(answer, &per_second(600), 600, &no_limits())
    }

    fn parse_with(answer: &str, spec: ChaptersSpec) -> Result<Vec<Chapter>, Vec<Violation>> {
        parse_answer(answer, &per_second(600), 600, &spec)
    }

    const NOT_A_TIME: &str = r#"chapter 2: "start" must be a time like "00:04:12""#;

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

    #[test]
    fn snap_nearest() {
        let segs = segments(&[0.0, 100.0, 200.0, 260.0, 400.0], 500.0);
        // 3:45 = 225 is 25 s from 200 and 35 s from 260; 4:05 = 245 is 45 s from 200, 15 s from 260.
        let a = answer(&[("0:00", "A"), ("3:45", "B"), ("4:05", "C")]);
        assert_eq!(
            parse_answer(&a, &segs, 500, &no_limits()).unwrap(),
            vec![chapter(0, "A"), chapter(200, "B"), chapter(260, "C")]
        );
    }

    #[test]
    fn snap_tie_earlier() {
        let segs = segments(&[0.0, 100.0, 200.0], 300.0);
        // 2:30 = 150 is 50 s from both 100 and 200.
        let a = answer(&[("0:00", "A"), ("2:30", "B"), ("3:20", "C")]);
        assert_eq!(
            parse_answer(&a, &segs, 300, &no_limits()).unwrap(),
            vec![chapter(0, "A"), chapter(100, "B"), chapter(200, "C")]
        );
    }

    #[test]
    fn snap_before_first() {
        let segs = segments(&[30.0, 100.0, 200.0], 300.0);
        let a = answer(&[("0:00", "A"), ("0:10", "B"), ("3:20", "C")]);
        assert_eq!(
            parse_answer(&a, &segs, 300, &no_limits()).unwrap(),
            vec![chapter(0, "A"), chapter(30, "B"), chapter(200, "C")]
        );
    }

    #[test]
    fn snap_after_last() {
        let segs = segments(&[0.0, 100.0, 200.0], 300.0);
        let a = answer(&[("0:00", "A"), ("1:40", "B"), ("4:50", "C")]);
        assert_eq!(
            parse_answer(&a, &segs, 300, &no_limits()).unwrap(),
            vec![chapter(0, "A"), chapter(100, "B"), chapter(200, "C")]
        );
    }

    #[test]
    fn snap_floors() {
        let segs = segments(&[0.0, 100.7, 200.2], 300.9);
        let a = answer(&[("0:00", "A"), ("1:41", "B"), ("3:20", "C")]);
        assert_eq!(
            parse_answer(&a, &segs, 300, &no_limits()).unwrap(),
            vec![chapter(0, "A"), chapter(100, "B"), chapter(200, "C")]
        );
    }

    #[test]
    fn snap_counts_segments_with_empty_text() {
        let mut segs = segments(&[0.0, 100.0, 200.0], 300.0);
        segs[1].text = String::new();
        let a = answer(&[("0:00", "A"), ("1:50", "B"), ("3:20", "C")]);
        assert_eq!(
            parse_answer(&a, &segs, 300, &no_limits()).unwrap(),
            vec![chapter(0, "A"), chapter(100, "B"), chapter(200, "C")]
        );
    }

    #[test]
    fn first_moves_to_zero() {
        let segs = segments(&[0.0, 15.0, 100.0, 200.0], 300.0);
        let a = answer(&[("0:15", "A"), ("1:40", "B"), ("3:20", "C")]);
        assert_eq!(
            parse_answer(&a, &segs, 300, &no_limits()).unwrap(),
            vec![chapter(0, "A"), chapter(100, "B"), chapter(200, "C")]
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
}
