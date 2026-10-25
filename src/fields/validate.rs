//! Parsing and validating a model answer against a fields spec.

use serde_json::{Map, Value as Json};

use super::{Field, FieldSpec, FieldsSpec, Kind, Value, Values, Violation};

/// `answer` is already cleaned by `call::clean`. All violations in one pass, in field order.
pub fn parse_answer(answer: &str, spec: &FieldsSpec) -> Result<Values, Vec<Violation>> {
    let object = parse_object(answer).map_err(|message| {
        vec![Violation(format!(
            "the answer is not a JSON object ({message})"
        ))]
    })?;
    let mut values = Vec::new();
    let mut violations = Vec::new();
    for field_spec in &spec.fields {
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
fn parse_object(text: &str) -> Result<Map<String, Json>, String> {
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
    };
    let mut found = Vec::new();
    let empty = match &value {
        Value::Text(text) => text.is_empty(),
        Value::List(items) => items.is_empty(),
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
                Value::List(_) => format!(
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
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fields::{Field, FieldSpec, Format, Limit, Value};

    fn spec(fields: &[(Field, Limit)]) -> FieldsSpec {
        FieldsSpec {
            fields: fields
                .iter()
                .map(|&(field, limit)| FieldSpec { field, limit })
                .collect(),
            format: Format::Plain,
        }
    }

    fn no_limit() -> Limit {
        Limit::default()
    }

    fn chars(n: usize) -> Limit {
        Limit {
            max_chars: Some(n),
            max: None,
        }
    }

    fn one(field: Field, limit: Limit) -> FieldsSpec {
        spec(&[(field, limit)])
    }

    fn messages(result: Result<Values, Vec<Violation>>) -> Vec<String> {
        result.unwrap_err().into_iter().map(|v| v.0).collect()
    }

    fn text(s: &str) -> Value {
        Value::Text(s.to_string())
    }

    fn list(items: &[&str]) -> Value {
        Value::List(items.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn parses_bare_object() {
        let s = one(Field::Title, no_limit());
        let v = parse_answer(r#"{"title": "Hello"}"#, &s).unwrap();
        assert_eq!(v, Values(vec![(Field::Title, text("Hello"))]));
    }

    #[test]
    fn parses_object_inside_prose() {
        let s = one(Field::Title, no_limit());
        let v = parse_answer(r#"Here you go: {"title": "Hello"} Enjoy!"#, &s).unwrap();
        assert_eq!(v, Values(vec![(Field::Title, text("Hello"))]));
    }

    #[test]
    fn not_json_violation() {
        let s = one(Field::Title, no_limit());
        assert_eq!(
            messages(parse_answer("I cannot do that", &s)),
            vec!["the answer is not a JSON object (expected value at line 1 column 1)"]
        );
    }

    #[test]
    fn not_json_violation_for_broken_object_in_prose() {
        let s = one(Field::Title, no_limit());
        let m = messages(parse_answer(r#"see {"title": } ok"#, &s));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn non_object_json_is_not_an_object() {
        let s = one(Field::Title, no_limit());
        for input in ["[1]", r#""x""#, "null", "1", "true", "", "{", "}{"] {
            let m = messages(parse_answer(input, &s));
            assert_eq!(m.len(), 1, "{input:?}");
            assert!(
                m[0].starts_with("the answer is not a JSON object ("),
                "{input:?}: {}",
                m[0]
            );
        }
    }

    #[test]
    fn violation_missing_key() {
        let s = one(Field::Title, no_limit());
        assert_eq!(
            messages(parse_answer(r#"{"body": "x"}"#, &s)),
            vec![r#"missing key "title""#]
        );
    }

    #[test]
    fn violation_not_a_string() {
        let s = one(Field::Title, no_limit());
        assert_eq!(
            messages(parse_answer(r#"{"title": 5}"#, &s)),
            vec![r#""title" must be a string"#]
        );
    }

    #[test]
    fn violation_not_a_list_of_strings() {
        let s = one(Field::Tags, no_limit());
        for input in [
            r#"{"tags": "a, b"}"#,
            r#"{"tags": [1]}"#,
            r#"{"tags": ["a", null]}"#,
            r#"{"tags": null}"#,
        ] {
            assert_eq!(
                messages(parse_answer(input, &s)),
                vec![r#""tags" must be a list of strings"#],
                "{input}"
            );
        }
    }

    #[test]
    fn violation_empty_text() {
        let s = one(Field::Title, no_limit());
        for input in [
            r#"{"title": ""}"#,
            r#"{"title": " \n\t "}"#,
            "{\"title\": \"\\u0001\"}",
        ] {
            assert_eq!(
                messages(parse_answer(input, &s)),
                vec![r#""title" is empty"#],
                "{input}"
            );
        }
    }

    #[test]
    fn violation_empty_list() {
        let s = one(Field::Tags, no_limit());
        for input in [r#"{"tags": []}"#, r#"{"tags": ["", "  "]}"#] {
            assert_eq!(
                messages(parse_answer(input, &s)),
                vec![r#""tags" is empty"#],
                "{input}"
            );
        }
        let h = one(Field::Hashtags, no_limit());
        assert_eq!(
            messages(parse_answer(r###"{"hashtags": ["#", "##"]}"###, &h)),
            vec![r#""hashtags" is empty"#]
        );
    }

    #[test]
    fn violation_hashtag_whitespace() {
        let s = one(Field::Hashtags, no_limit());
        assert_eq!(
            messages(parse_answer(
                r##"{"hashtags": ["ok", "#machine learning", "a\tb"]}"##,
                &s
            )),
            vec![
                r#"hashtag "machine learning" contains whitespace"#,
                r#"hashtag "a\tb" contains whitespace"#,
            ]
        );
    }

    #[test]
    fn violation_over_max_chars_text() {
        let s = one(Field::Body, chars(5));
        assert_eq!(
            messages(parse_answer(r#"{"body": "abcdefgh"}"#, &s)),
            vec![r#""body" has 8 characters; the limit is 5. Shorten it to at most 5 characters."#]
        );
    }

    #[test]
    fn violation_over_max() {
        let s = one(
            Field::Hashtags,
            Limit {
                max_chars: None,
                max: Some(2),
            },
        );
        assert_eq!(
            messages(parse_answer(r#"{"hashtags": ["a", "b", "c"]}"#, &s)),
            vec![r#""hashtags" has 3 items; the limit is 2. Keep the 2 most relevant."#]
        );
    }

    #[test]
    fn violation_over_max_chars_rendered_list() {
        let s = one(Field::Hashtags, chars(5));
        // rendered: "#ab #cd" = 7 characters
        assert_eq!(
            messages(parse_answer(r#"{"hashtags": ["ab", "cd"]}"#, &s)),
            vec![
                r#""hashtags" is 7 characters as rendered; the limit is 5. Use fewer or shorter hashtags."#
            ]
        );
    }

    #[test]
    fn violations_in_field_order() {
        let s = spec(&[
            (Field::Title, no_limit()),
            (Field::Body, chars(2)),
            (Field::Tags, no_limit()),
            (Field::ThumbnailText, no_limit()),
        ]);
        let answer = r#"{"body": "abc", "tags": "x", "title": ""}"#;
        assert_eq!(
            messages(parse_answer(answer, &s)),
            vec![
                r#""title" is empty"#.to_string(),
                r#""body" has 3 characters; the limit is 2. Shorten it to at most 2 characters."#
                    .to_string(),
                r#""tags" must be a list of strings"#.to_string(),
                r#"missing key "thumbnail_text""#.to_string(),
            ]
        );
    }

    #[test]
    fn violations_within_a_field_follow_the_documented_order() {
        let s = one(
            Field::Hashtags,
            Limit {
                max_chars: Some(3),
                max: Some(1),
            },
        );
        assert_eq!(
            messages(parse_answer(r#"{"hashtags": ["a b", "cd"]}"#, &s)),
            vec![
                r#"hashtag "a b" contains whitespace"#.to_string(),
                r#""hashtags" has 2 items; the limit is 1. Keep the 1 most relevant."#.to_string(),
                r#""hashtags" is 8 characters as rendered; the limit is 3. Use fewer or shorter hashtags."#
                    .to_string(),
            ]
        );
    }

    #[test]
    fn max_chars_boundary_inclusive() {
        let s = one(Field::Title, chars(5));
        assert!(parse_answer(r#"{"title": "abcde"}"#, &s).is_ok());
        assert_eq!(
            messages(parse_answer(r#"{"title": "abcdef"}"#, &s)),
            vec![
                r#""title" has 6 characters; the limit is 5. Shorten it to at most 5 characters."#
            ]
        );
        let h = one(Field::Hashtags, chars(7));
        assert!(parse_answer(r#"{"hashtags": ["ab", "cd"]}"#, &h).is_ok());
        let h = one(Field::Hashtags, chars(6));
        assert!(parse_answer(r#"{"hashtags": ["ab", "cd"]}"#, &h).is_err());
    }

    #[test]
    fn multibyte_counts_as_one() {
        let s = one(Field::Title, chars(2));
        assert!(parse_answer(r#"{"title": "é🎉"}"#, &s).is_ok());
        assert!(parse_answer(r#"{"title": "é🎉x"}"#, &s).is_err());
    }

    #[test]
    fn max_chars_is_measured_after_normalization() {
        let s = one(Field::Title, chars(3));
        // CRLF becomes one character, controls vanish, surrounding space is trimmed.
        assert!(parse_answer("{\"title\": \"  a\\r\\nb\\u0001 \"}", &s).is_ok());
    }

    #[test]
    fn normalizes_crlf() {
        let s = one(Field::Body, no_limit());
        let v = parse_answer(r#"{"body": "a\r\nb\r\nc"}"#, &s).unwrap();
        assert_eq!(v, Values(vec![(Field::Body, text("a\nb\nc"))]));
    }

    #[test]
    fn strips_control_chars() {
        let s = one(Field::Body, no_limit());
        let v = parse_answer(r#"{"body": "a\u0000b\u0007c\u001bd\u007fe\tf\ng"}"#, &s).unwrap();
        assert_eq!(v, Values(vec![(Field::Body, text("abcde\tf\ng"))]));
    }

    #[test]
    fn strips_c1_control_chars() {
        let s = one(Field::Body, no_limit());
        for input in [
            r#"{"body": "a\u0080b"}"#,
            r#"{"body": "a\u008db"}"#,
            r#"{"body": "a\u009bb"}"#,
            r#"{"body": "a\u009fb"}"#,
        ] {
            let v = parse_answer(input, &s).unwrap();
            assert_eq!(v, Values(vec![(Field::Body, text("ab"))]), "{input}");
        }
    }

    #[test]
    fn trims_values() {
        let s = spec(&[(Field::Title, no_limit()), (Field::Tags, no_limit())]);
        let v = parse_answer(r#"{"title": "  \n hi \t", "tags": ["  a ", "b\n"]}"#, &s).unwrap();
        assert_eq!(
            v,
            Values(vec![
                (Field::Title, text("hi")),
                (Field::Tags, list(&["a", "b"]))
            ])
        );
    }

    #[test]
    fn drops_empty_list_items() {
        let s = one(Field::Tags, no_limit());
        let v = parse_answer(r#"{"tags": ["a", "", "  ", "\u0001", "b"]}"#, &s).unwrap();
        assert_eq!(v, Values(vec![(Field::Tags, list(&["a", "b"]))]));
    }

    #[test]
    fn strips_leading_hashes() {
        let s = one(Field::Hashtags, no_limit());
        let v = parse_answer(
            r###"{"hashtags": ["##rust", "#go", "c#", "#", "# zig"]}"###,
            &s,
        )
        .unwrap();
        assert_eq!(
            v,
            Values(vec![(Field::Hashtags, list(&["rust", "go", "c#", "zig"]))])
        );
    }

    #[test]
    fn keeps_hashes_in_tags() {
        let s = one(Field::Tags, no_limit());
        let v = parse_answer(r##"{"tags": ["#a"]}"##, &s).unwrap();
        assert_eq!(v, Values(vec![(Field::Tags, list(&["#a"]))]));
    }

    #[test]
    fn ignores_undeclared_keys() {
        let s = one(Field::Title, no_limit());
        let v = parse_answer(r#"{"extra": 1, "title": "T", "body": ["x"]}"#, &s).unwrap();
        assert_eq!(v, Values(vec![(Field::Title, text("T"))]));
    }

    #[test]
    fn values_follow_declared_order() {
        let s = spec(&[(Field::Tags, no_limit()), (Field::Title, no_limit())]);
        let v = parse_answer(r#"{"title": "T", "tags": ["a"]}"#, &s).unwrap();
        assert_eq!(
            v,
            Values(vec![(Field::Tags, list(&["a"])), (Field::Title, text("T"))])
        );
    }
}
