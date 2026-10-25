//! Rendering validated values as `.md` and `.json` artifacts.

use serde::ser::{SerializeMap, Serializer};
use serde::Serialize;

use super::{Field, Format, Value, Values};

/// The `<name>.md` text: declared fields separated by a blank line, one trailing newline.
pub fn render(values: &Values, format: Format) -> String {
    let blocks: Vec<String> = values
        .0
        .iter()
        .map(|(field, value)| match (format, field, value) {
            (Format::Markdown, Field::Title, Value::Text(title)) => format!("# {title}"),
            _ => value.rendered(*field),
        })
        .collect();
    format!("{}\n", blocks.join("\n\n"))
}

/// The `<name>.json` artifact (ADR-0007), pretty-printed with a trailing newline.
pub fn artifact_json(template: &str, output_lang_code: &str, values: &Values) -> String {
    let artifact = Artifact {
        version: 1,
        template,
        output_lang: output_lang_code,
        fields: FieldsJson(values),
    };
    let mut json = serde_json::to_string_pretty(&artifact).expect("artifact serializes");
    json.push('\n');
    json
}

#[derive(Serialize)]
struct Artifact<'a> {
    version: u32,
    template: &'a str,
    output_lang: &'a str,
    fields: FieldsJson<'a>,
}

/// Serializes the values as an object in declared order (`serde_json::Map` would sort keys).
struct FieldsJson<'a>(&'a Values);

impl Serialize for FieldsJson<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0 .0.len()))?;
        for (field, value) in &self.0 .0 {
            match value {
                Value::Text(text) => map.serialize_entry(field.name(), text)?,
                Value::List(items) => map.serialize_entry(field.name(), items)?,
            }
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fields::{Field, Value};

    fn list(items: &[&str]) -> Value {
        Value::List(items.iter().map(|s| s.to_string()).collect())
    }

    fn text(s: &str) -> Value {
        Value::Text(s.to_string())
    }

    fn all_fields() -> Values {
        Values(vec![
            (Field::Title, text("My title")),
            (Field::Body, text("Line one\nLine two")),
            (Field::Tags, list(&["rust", "cli"])),
            (Field::Hashtags, list(&["rust", "clases"])),
            (Field::ThumbnailText, text("Big news")),
        ])
    }

    #[test]
    fn render_plain_text_fields() {
        let values = Values(vec![
            (Field::Title, text("My title")),
            (Field::Body, text("Line one\nLine two")),
            (Field::ThumbnailText, text("Big news")),
        ]);
        assert_eq!(
            render(&values, Format::Plain),
            "My title\n\nLine one\nLine two\n\nBig news\n"
        );
    }

    #[test]
    fn render_plain_tags() {
        let values = Values(vec![(Field::Tags, list(&["rust", "cli"]))]);
        assert_eq!(render(&values, Format::Plain), "rust, cli\n");
    }

    #[test]
    fn render_plain_hashtags() {
        let values = Values(vec![(Field::Hashtags, list(&["rust", "clases"]))]);
        assert_eq!(render(&values, Format::Plain), "#rust #clases\n");
    }

    #[test]
    fn render_plain_keeps_declared_order() {
        let values = Values(vec![
            (Field::Hashtags, list(&["a"])),
            (Field::Body, text("b")),
        ]);
        assert_eq!(render(&values, Format::Plain), "#a\n\nb\n");
    }

    #[test]
    fn render_markdown_title_heading() {
        assert_eq!(
            render(&all_fields(), Format::Markdown),
            "# My title\n\nLine one\nLine two\n\nrust, cli\n\n#rust #clases\n\nBig news\n"
        );
    }

    #[test]
    fn render_markdown_without_title_is_plain() {
        let values = Values(vec![(Field::Body, text("b"))]);
        assert_eq!(render(&values, Format::Markdown), "b\n");
    }

    #[test]
    fn render_trailing_newline() {
        let values = Values(vec![(Field::Body, text("b")), (Field::Title, text("t"))]);
        for format in [Format::Plain, Format::Markdown] {
            let out = render(&values, format);
            assert!(out.ends_with("t\n"), "{out:?}");
            assert!(!out.ends_with("\n\n"), "{out:?}");
        }
    }

    #[test]
    fn artifact_json_golden() {
        let values = Values(vec![
            (Field::Title, text("My title")),
            (Field::Tags, list(&["rust", "cli"])),
            (Field::Hashtags, list(&[])),
        ]);
        let expected = "{\n  \"version\": 1,\n  \"template\": \"youtube\",\n  \"output_lang\": \"es\",\n  \"fields\": {\n    \"title\": \"My title\",\n    \"tags\": [\n      \"rust\",\n      \"cli\"\n    ],\n    \"hashtags\": []\n  }\n}\n";
        assert_eq!(artifact_json("youtube", "es", &values), expected);
    }

    #[test]
    fn artifact_json_round_trips_with_only_given_fields() {
        let values = Values(vec![
            (Field::Hashtags, list(&["a"])),
            (Field::Body, text("quote \" and \n newline")),
        ]);
        let parsed: serde_json::Value =
            serde_json::from_str(&artifact_json("t", "en", &values)).unwrap();
        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["template"], "t");
        assert_eq!(parsed["output_lang"], "en");
        assert_eq!(
            parsed["fields"],
            serde_json::json!({"hashtags": ["a"], "body": "quote \" and \n newline"})
        );
    }
}
