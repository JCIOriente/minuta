//! Chapter validation: parsing and checking the model's chapters (ADR-0008).

use serde_json::{Map, Value as Json};

use super::{lines, parse_stamp, stamp, Chapter, ChaptersSpec, MIN_CHAPTERS, MIN_SECONDS};
use crate::fields::validate::{normalize, parse_object};
use crate::fields::Violation;
use crate::transcript::Segment;

mod snap;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests_edges;
#[cfg(test)]
mod tests_rules;

use snap::snap;

/// `answer` is already cleaned by `call::clean`. `end` = last segment end, floored. Snaps, then
/// checks the rules; all violations in one pass, in the spec's order. With no segments there is
/// nothing to snap to: the starts stay as parsed (the first one still becomes 0).
pub(crate) fn parse_answer(
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
