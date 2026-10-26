//! Prompt blocks, repair prompts and the JSON schema for a fields spec.

use serde_json::{json, Value as Json};

use super::{Field, FieldSpec, FieldsSpec, Format, Kind, Limit, Violation};
use crate::llm::chunk::language_instruction;

pub(crate) const REPAIR_INTRO: &str = "You correct a JSON object. Fix only the problems listed in the input, keep everything else unchanged, and return only the corrected JSON object.";
const BLOCK_INTRO: &str =
    "Return only a JSON object with exactly these keys, and no text before or after it:";
const PLAIN_SENTENCE: &str = "Write the values as plain text without Markdown.";
const MARKDOWN_SENTENCE: &str = "`body` may use Markdown; the other values are plain text.";
const KEYS_SENTENCE: &str =
    "Keep the keys exactly as written, in English; write the values in the language given below.";

/// The fields block of the final and repair system prompts (no language instruction). Filled
/// fields are left out.
pub fn fields_block(spec: &FieldsSpec) -> String {
    let mut lines = vec![BLOCK_INTRO.to_string()];
    lines.extend(spec.fields.iter().filter_map(field_line));
    let format_sentence = match spec.format {
        Format::Plain => PLAIN_SENTENCE,
        Format::Markdown => MARKDOWN_SENTENCE,
    };
    lines.push(format!("{format_sentence} {KEYS_SENTENCE}"));
    lines.join("\n")
}

/// `None` for a filled field: the model is not asked for it.
fn field_line(spec: &FieldSpec) -> Option<String> {
    let field = spec.field;
    let kind = match field.kind() {
        Kind::Text => "string",
        Kind::List => "array of strings",
        Kind::Chapters => return None,
    };
    let mut line = format!("\"{}\" ({kind}): {}", field.name(), field.description());
    if let Some(limits) = limit_sentence(spec) {
        line.push(' ');
        line.push_str(&limits);
    }
    Some(line)
}

fn limit_sentence(spec: &FieldSpec) -> Option<String> {
    let Limit { max_chars, max } = spec.limit;
    match spec.field.kind() {
        Kind::Text => return max_chars.map(|n| format!("At most {n} characters.")),
        Kind::Chapters => return None,
        Kind::List => {}
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

/// JSON schema for structured output: the declared fields the model answers (filled fields
/// are left out), no length constraints.
pub fn schema(spec: &FieldsSpec) -> Json {
    let asked: Vec<(&str, Json)> = spec
        .fields
        .iter()
        .filter_map(|f| {
            let property = match f.field.kind() {
                Kind::Text => json!({"type": "string"}),
                Kind::List => json!({"type": "array", "items": {"type": "string"}}),
                Kind::Chapters => return None,
            };
            Some((f.field.name(), property))
        })
        .collect();
    let required: Vec<&str> = asked.iter().map(|(name, _)| *name).collect();
    let properties: serde_json::Map<String, Json> = asked
        .into_iter()
        .map(|(name, property)| (name.to_string(), property))
        .collect();
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

    fn youtube_spec(fields: &[Field]) -> FieldsSpec {
        let limit = |max_chars, max| Limit { max_chars, max };
        FieldsSpec {
            fields: fields
                .iter()
                .map(|&field| FieldSpec {
                    field,
                    limit: match field {
                        Field::Title => limit(Some(100), None),
                        Field::Body => limit(Some(5000), None),
                        Field::Tags => limit(Some(500), Some(15)),
                        Field::ThumbnailText => limit(Some(40), None),
                        _ => no_limit(),
                    },
                })
                .collect(),
            format: Format::Plain,
        }
    }

    const WITH_CHAPTERS: [Field; 5] = [
        Field::Title,
        Field::Body,
        Field::Chapters,
        Field::Tags,
        Field::ThumbnailText,
    ];

    #[test]
    fn block_skips_chapters() {
        let expected = "Return only a JSON object with exactly these keys, and no text before or after it:\n\
\"title\" (string): A title. At most 100 characters.\n\
\"body\" (string): The main text. At most 5000 characters.\n\
\"tags\" (array of strings): Search keywords, without `#`. At most 15 items, at most 500 characters joined with \", \".\n\
\"thumbnail_text\" (string): A few words to show on a thumbnail image. At most 40 characters.\n\
Write the values as plain text without Markdown. Keep the keys exactly as written, in English; write the values in the language given below.";
        assert_eq!(fields_block(&youtube_spec(&WITH_CHAPTERS)), expected);
        assert_eq!(
            fields_block(&youtube_spec(&[
                Field::Title,
                Field::Body,
                Field::Tags,
                Field::ThumbnailText
            ])),
            expected
        );
    }

    #[test]
    fn schema_skips_chapters() {
        let expected = json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "body": {"type": "string"},
                "tags": {"type": "array", "items": {"type": "string"}},
                "thumbnail_text": {"type": "string"}
            },
            "required": ["title", "body", "tags", "thumbnail_text"]
        });
        assert_eq!(schema(&youtube_spec(&WITH_CHAPTERS)), expected);
        assert_eq!(
            schema(&youtube_spec(&[
                Field::Title,
                Field::Body,
                Field::Tags,
                Field::ThumbnailText
            ])),
            expected
        );
    }
}
