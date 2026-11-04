//! Helpers shared by the chapter validation test modules.

use serde_json::json;

use super::parse_answer;
use crate::chapters::{Chapter, ChaptersSpec};
use crate::fields::Violation;
use crate::transcript::Segment;

fn seg(start: f64, end: f64) -> Segment {
    Segment {
        start,
        end,
        text: "x".into(),
    }
}

/// Segments starting at `starts`, each ending where the next one starts, the last at `end`.
pub(super) fn segments(starts: &[f64], end: f64) -> Vec<Segment> {
    starts
        .iter()
        .enumerate()
        .map(|(i, &start)| seg(start, starts.get(i + 1).copied().unwrap_or(end)))
        .collect()
}

/// One-second segments from 0 to `end`, so whole-second starts snap to themselves.
pub(super) fn per_second(end: u64) -> Vec<Segment> {
    (0..end).map(|s| seg(s as f64, (s + 1) as f64)).collect()
}

pub(super) fn answer(chapters: &[(&str, &str)]) -> String {
    let items: Vec<_> = chapters
        .iter()
        .map(|(start, title)| json!({"start": start, "title": title}))
        .collect();
    json!({ "chapters": items }).to_string()
}

pub(super) fn chapter(start: u64, title: &str) -> Chapter {
    Chapter {
        start,
        title: title.into(),
    }
}

pub(super) fn no_limits() -> ChaptersSpec {
    ChaptersSpec::default()
}

pub(super) fn messages(result: Result<Vec<Chapter>, Vec<Violation>>) -> Vec<String> {
    result.unwrap_err().into_iter().map(|v| v.0).collect()
}

/// One-second segments over 10 minutes, no limits.
pub(super) fn parse(answer: &str) -> Result<Vec<Chapter>, Vec<Violation>> {
    parse_answer(answer, &per_second(600), 600, &no_limits())
}

pub(super) fn parse_with(answer: &str, spec: ChaptersSpec) -> Result<Vec<Chapter>, Vec<Violation>> {
    parse_answer(answer, &per_second(600), 600, &spec)
}

pub(super) const NOT_A_TIME: &str = r#"chapter 2: "start" must be a time like "00:04:12""#;
