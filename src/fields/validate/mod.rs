//! Parsing and validating a model answer against a fields spec.

use serde_json::{Map, Value as Json};

use super::{Field, FieldSpec, FieldsSpec, Kind, Value, Values, Violation};

/// `answer` is already cleaned by `call::clean`. All violations in one pass, in field order.
/// Filled fields are not part of the answer: a missing key is fine, a present one is ignored.
pub(crate) fn parse_answer(answer: &str, spec: &FieldsSpec) -> Result<Values, Vec<Violation>> {
    let object = parse_object(answer).map_err(|message| {
        vec![Violation(format!(
            "the answer is not a JSON object ({message})"
        ))]
    })?;
    let mut values = Vec::new();
    let mut violations = Vec::new();
    for field_spec in spec.fields.iter().filter(|f| !f.field.is_filled()) {
        match check_field(&object, field_spec) {
            Ok(value) => values.push((field_spec.field, value)),
            Err(found) => violations.extend(found),
        }
    }
    if violations.is_empty() {
        Ok(Values(values))
    } else {
        Err(violations)
    }
}

/// The whole text, else the text from the first `{` to the last `}`. On failure, the message
/// of the last attempt (the more specific one when there is an object-like span).
pub(crate) fn parse_object(text: &str) -> Result<Map<String, Json>, String> {
    let mut message = match parse_whole(text) {
        Ok(object) => return Ok(object),
        Err(message) => message,
    };
    if let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) {
        if start < end {
            match parse_whole(&text[start..=end]) {
                Ok(object) => return Ok(object),
                Err(second) => message = second,
            }
        }
    }
    Err(message)
}

fn parse_whole(text: &str) -> Result<Map<String, Json>, String> {
    match serde_json::from_str::<Json>(text) {
        Ok(Json::Object(object)) => Ok(object),
        Ok(other) => Err(format!("found {} instead", json_kind(&other))),
        Err(err) => Err(err.to_string()),
    }
}

fn json_kind(value: &Json) -> &'static str {
    match value {
        Json::Null => "null",
        Json::Bool(_) => "a boolean",
        Json::Number(_) => "a number",
        Json::String(_) => "a string",
        Json::Array(_) => "an array",
        Json::Object(_) => "an object",
    }
}

fn check_field(
    object: &Map<String, Json>,
    field_spec: &FieldSpec,
) -> Result<Value, Vec<Violation>> {
    let field = field_spec.field;
    let name = field.name();
    let violation = |message: String| Err(vec![Violation(message)]);
    let Some(raw) = object.get(name) else {
        return violation(format!("missing key \"{name}\""));
    };
    let value = match field.kind() {
        Kind::Text => match raw {
            Json::String(s) => Value::Text(normalize(s)),
            _ => return violation(format!("\"{name}\" must be a string")),
        },
        Kind::List => match string_items(raw) {
            Some(items) => Value::List(list_items(field, items)),
            None => return violation(format!("\"{name}\" must be a list of strings")),
        },
        Kind::Chapters => return violation(format!("\"{name}\" is filled by minuta")),
    };
    let mut found = Vec::new();
    let empty = match &value {
        Value::Text(text) => text.is_empty(),
        Value::List(items) => items.is_empty(),
        Value::Chapters(chapters) => chapters.is_empty(),
    };
    if empty {
        return violation(format!("\"{name}\" is empty"));
    }
    if let Value::List(items) = &value {
        if field == Field::Hashtags {
            for item in items.iter().filter(|i| i.chars().any(char::is_whitespace)) {
                found.push(format!("hashtag {item:?} contains whitespace"));
            }
        }
        if let Some(max) = field_spec.limit.max {
            if items.len() > max {
                found.push(format!(
                    "\"{name}\" has {} items; the limit is {max}. Keep the {max} most relevant.",
                    items.len()
                ));
            }
        }
    }
    if let Some(max_chars) = field_spec.limit.max_chars {
        let count = value.rendered(field).chars().count();
        if count > max_chars {
            found.push(match value {
                Value::Text(_) => format!(
                    "\"{name}\" has {count} characters; the limit is {max_chars}. Shorten it to at most {max_chars} characters."
                ),
                Value::List(_) | Value::Chapters(_) => format!(
                    "\"{name}\" is {count} characters as rendered; the limit is {max_chars}. Use fewer or shorter {name}."
                ),
            });
        }
    }
    if found.is_empty() {
        Ok(value)
    } else {
        Err(found.into_iter().map(Violation).collect())
    }
}

fn string_items(raw: &Json) -> Option<Vec<&str>> {
    raw.as_array()?.iter().map(Json::as_str).collect()
}

/// Normalizes and trims each item, strips leading `#` from hashtags, drops empty items.
fn list_items(field: Field, items: Vec<&str>) -> Vec<String> {
    items
        .into_iter()
        .map(|item| {
            let item = normalize(item);
            if field == Field::Hashtags {
                item.trim_start_matches('#').trim().to_string()
            } else {
                item
            }
        })
        .filter(|item| !item.is_empty())
        .collect()
}

/// CRLF to LF, drops every control character (C0, DEL, C1) other than `\n` and `\t`, then trims.
pub(crate) fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests;
