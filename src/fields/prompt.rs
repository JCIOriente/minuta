//! Prompt blocks, repair prompts and the JSON schema for a fields spec.

use serde_json::{json, Value as Json};

use super::{Field, FieldSpec, FieldsSpec, Format, Kind, Limit, Violation};
use crate::llm::chunk::language_instruction;

const REPAIR_INTRO: &str = "You correct a JSON object. Fix only the problems listed in the input, keep everything else unchanged, and return only the corrected JSON object.";
const BLOCK_INTRO: &str =
    "Return only a JSON object with exactly these keys, and no text before or after it:";
const PLAIN_SENTENCE: &str = "Write the values as plain text without Markdown.";
const MARKDOWN_SENTENCE: &str = "`body` may use Markdown; the other values are plain text.";
const KEYS_SENTENCE: &str =
    "Keep the keys exactly as written, in English; write the values in the language given below.";

/// The fields block of the final and repair system prompts (no language instruction).
pub fn fields_block(spec: &FieldsSpec) -> String {
    let mut lines = vec![BLOCK_INTRO.to_string()];
    lines.extend(spec.fields.iter().map(field_line));
    let format_sentence = match spec.format {
        Format::Plain => PLAIN_SENTENCE,
        Format::Markdown => MARKDOWN_SENTENCE,
    };
    lines.push(format!("{format_sentence} {KEYS_SENTENCE}"));
    lines.join("\n")
}

fn field_line(spec: &FieldSpec) -> String {
    let field = spec.field;
    let kind = match field.kind() {
        Kind::Text => "string",
        Kind::List => "array of strings",
    };
    let mut line = format!("\"{}\" ({kind}): {}", field.name(), field.description());
    if let Some(limits) = limit_sentence(spec) {
        line.push(' ');
        line.push_str(&limits);
    }
    line
}

fn limit_sentence(spec: &FieldSpec) -> Option<String> {
    let Limit { max_chars, max } = spec.limit;
    if spec.field.kind() == Kind::Text {
        return max_chars.map(|n| format!("At most {n} characters."));
    }
    let joined = if spec.field == Field::Hashtags {
        "joined with \" \" and prefixed with #"
    } else {
        "joined with \", \""
    };
    match (max, max_chars) {
        (None, None) => None,
        (Some(n), None) => Some(format!("At most {n} items.")),
        (None, Some(m)) => Some(format!("At most {m} characters {joined}.")),
        (Some(n), Some(m)) => Some(format!(
            "At most {n} items, at most {m} characters {joined}."
        )),
    }
}

/// System prompt of a repair call; the language instruction is last.
pub fn repair_system(spec: &FieldsSpec, output_lang: &str) -> String {
    format!(
        "{REPAIR_INTRO}\n\n{}\n\n{}",
        fields_block(spec),
        language_instruction(output_lang)
    )
}

/// User message of a repair call: the previous answer and what is wrong with it.
pub fn repair_user(previous: &str, violations: &[Violation]) -> String {
    let problems: Vec<String> = violations.iter().map(|v| format!("- {v}")).collect();
    format!(
        "Previous answer:\n{previous}\n\nProblems:\n{}",
        problems.join("\n")
    )
}

/// JSON schema for structured output: the declared fields, no length constraints.
pub fn schema(spec: &FieldsSpec) -> Json {
    let properties: serde_json::Map<String, Json> = spec
        .fields
        .iter()
        .map(|f| {
            let property = match f.field.kind() {
                Kind::Text => json!({"type": "string"}),
                Kind::List => json!({"type": "array", "items": {"type": "string"}}),
            };
            (f.field.name().to_string(), property)
        })
        .collect();
    let required: Vec<&str> = spec.fields.iter().map(|f| f.field.name()).collect();
    json!({"type": "object", "properties": properties, "required": required})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_of(fields: &[(Field, Limit)], format: Format) -> FieldsSpec {
        FieldsSpec {
            fields: fields
                .iter()
                .map(|&(field, limit)| FieldSpec { field, limit })
                .collect(),
            format,
        }
    }

    fn no_limit() -> Limit {
        Limit::default()
    }

    #[test]
    fn fields_block_lists_fields_in_order() {
        let spec = spec_of(
            &[(Field::Tags, no_limit()), (Field::Title, no_limit())],
            Format::Plain,
        );
        let expected = "Return only a JSON object with exactly these keys, and no text before or after it:\n\
\"tags\" (array of strings): Search keywords, without `#`.\n\
\"title\" (string): A title.\n\
Write the values as plain text without Markdown. Keep the keys exactly as written, in English; write the values in the language given below.";
        assert_eq!(fields_block(&spec), expected);
    }

    #[test]
    fn fields_block_text_limit() {
        let limit = Limit {
            max_chars: Some(100),
            max: None,
        };
        let block = fields_block(&spec_of(&[(Field::Title, limit)], Format::Plain));
        assert!(block.contains("\"title\" (string): A title. At most 100 characters.\n"));
    }

    #[test]
    fn fields_block_tags_limit() {
        let limit = Limit {
            max_chars: Some(500),
            max: Some(15),
        };
        let block = fields_block(&spec_of(&[(Field::Tags, limit)], Format::Plain));
        assert!(block.contains(
            "\"tags\" (array of strings): Search keywords, without `#`. At most 15 items, at most 500 characters joined with \", \".\n"
        ));
    }

    #[test]
    fn fields_block_tags_single_limits() {
        let only_max = Limit {
            max_chars: None,
            max: Some(5),
        };
        let block = fields_block(&spec_of(&[(Field::Tags, only_max)], Format::Plain));
        assert!(block.contains("without `#`. At most 5 items.\n"));
        let only_chars = Limit {
            max_chars: Some(80),
            max: None,
        };
        let block = fields_block(&spec_of(&[(Field::Tags, only_chars)], Format::Plain));
        assert!(block.contains("without `#`. At most 80 characters joined with \", \".\n"));
    }

    #[test]
    fn fields_block_hashtags_limit() {
        let limit = Limit {
            max_chars: Some(150),
            max: Some(5),
        };
        let block = fields_block(&spec_of(&[(Field::Hashtags, limit)], Format::Plain));
        assert!(block.contains(
            "\"hashtags\" (array of strings): Hashtags, written without the `#` sign. At most 5 items, at most 150 characters joined with \" \" and prefixed with #.\n"
        ));
    }

    #[test]
    fn fields_block_plain_sentence() {
        let block = fields_block(&spec_of(&[(Field::Body, no_limit())], Format::Plain));
        assert!(block.contains("\nWrite the values as plain text without Markdown. Keep the keys"));
        assert!(!block.contains("may use Markdown"));
    }

    #[test]
    fn fields_block_markdown_sentence() {
        let block = fields_block(&spec_of(&[(Field::Body, no_limit())], Format::Markdown));
        assert!(block.contains(
            "\n`body` may use Markdown; the other values are plain text. Keep the keys exactly as written, in English; write the values in the language given below."
        ));
        assert!(!block.contains("without Markdown"));
    }

    #[test]
    fn repair_system_language_last() {
        let spec = spec_of(&[(Field::Title, no_limit())], Format::Plain);
        let expected = format!(
            "You correct a JSON object. Fix only the problems listed in the input, keep everything else unchanged, and return only the corrected JSON object.\n\n\
Return only a JSON object with exactly these keys, and no text before or after it:\n\
\"title\" (string): A title.\n\
Write the values as plain text without Markdown. Keep the keys exactly as written, in English; write the values in the language given below.\n\n{}",
            language_instruction("Spanish")
        );
        assert_eq!(repair_system(&spec, "Spanish"), expected);
    }

    #[test]
    fn repair_user_layout() {
        let violations = [Violation("a".into()), Violation("b".into())];
        assert_eq!(
            repair_user("{\"title\":\"x\"}", &violations),
            "Previous answer:\n{\"title\":\"x\"}\n\nProblems:\n- a\n- b"
        );
    }

    #[test]
    fn schema_text_and_list() {
        let spec = spec_of(
            &[
                (
                    Field::Title,
                    Limit {
                        max_chars: Some(5),
                        max: None,
                    },
                ),
                (
                    Field::Tags,
                    Limit {
                        max_chars: Some(9),
                        max: Some(2),
                    },
                ),
            ],
            Format::Plain,
        );
        assert_eq!(
            schema(&spec),
            json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string"},
                    "tags": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["title", "tags"]
            })
        );
    }

    #[test]
    fn schema_required_in_order() {
        let spec = spec_of(
            &[
                (Field::ThumbnailText, no_limit()),
                (Field::Body, no_limit()),
                (Field::Hashtags, no_limit()),
            ],
            Format::Plain,
        );
        assert_eq!(
            schema(&spec)["required"],
            json!(["thumbnail_text", "body", "hashtags"])
        );
    }
}
