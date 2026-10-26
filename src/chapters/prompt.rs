//! Chapter prompts: the chapters block and its repair prompt (ADR-0008).

use serde_json::Value as Json;

use super::ChaptersSpec;

/// The chapters block of the final and repair system prompts (no language instruction).
pub fn chapters_block(spec: &ChaptersSpec) -> String {
    todo!("chapters_block {spec:?}")
}

/// System prompt of a repair call; the language instruction is last.
pub fn repair_system(spec: &ChaptersSpec, output_lang: &str) -> String {
    todo!("repair_system {spec:?} {output_lang}")
}

/// JSON schema for structured output: the `chapters` list, no length constraints.
pub fn schema() -> Json {
    todo!("schema")
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILT_IN: ChaptersSpec = ChaptersSpec {
        max: Some(20),
        max_chars: Some(1500),
    };

    #[test]
    fn block_builtin_limits() {
        let expected = r#"Return only a JSON object with one key, "chapters", and no text before or after it. "chapters" is an array of objects, one per chapter, in the order they happen:
"start" (string): when the chapter starts, copied from a timestamp in the input, like "00:04:12". The first chapter starts at "00:00:00".
"title" (string): the chapter title, on one line, in plain text without Markdown.
Give at least 3 and at most 20 chapters. Each chapter lasts at least 10 seconds. As "0:00 Title" lines, all chapters together have at most 1500 characters.
Keep the keys exactly as written, in English; write the titles in the language given below."#;
        assert_eq!(chapters_block(&BUILT_IN), expected);
    }

    #[test]
    fn block_without_max() {
        let spec = ChaptersSpec {
            max: None,
            max_chars: Some(1500),
        };
        assert_eq!(
            chapters_block(&spec).lines().nth(3),
            Some(r#"Give at least 3 chapters. Each chapter lasts at least 10 seconds. As "0:00 Title" lines, all chapters together have at most 1500 characters."#)
        );
    }

    #[test]
    fn block_without_max_chars() {
        let spec = ChaptersSpec {
            max: Some(20),
            max_chars: None,
        };
        assert_eq!(
            chapters_block(&spec).lines().nth(3),
            Some("Give at least 3 and at most 20 chapters. Each chapter lasts at least 10 seconds.")
        );
    }

    #[test]
    fn block_without_limits() {
        assert_eq!(
            chapters_block(&ChaptersSpec::default()).lines().nth(3),
            Some("Give at least 3 chapters. Each chapter lasts at least 10 seconds.")
        );
    }

    #[test]
    fn repair_system_layout() {
        let expected = r#"You correct a JSON object. Fix only the problems listed in the input, keep everything else unchanged, and return only the corrected JSON object.

Return only a JSON object with one key, "chapters", and no text before or after it. "chapters" is an array of objects, one per chapter, in the order they happen:
"start" (string): when the chapter starts, copied from a timestamp in the input, like "00:04:12". The first chapter starts at "00:00:00".
"title" (string): the chapter title, on one line, in plain text without Markdown.
Give at least 3 and at most 20 chapters. Each chapter lasts at least 10 seconds. As "0:00 Title" lines, all chapters together have at most 1500 characters.
Keep the keys exactly as written, in English; write the titles in the language given below.

Write the entire response in Spanish, including headings and any placeholder text, regardless of the language of the input or of these instructions."#;
        assert_eq!(repair_system(&BUILT_IN, "Spanish"), expected);
    }

    #[test]
    fn schema_exact() {
        assert_eq!(
            serde_json::to_string(&schema()).unwrap(),
            r#"{"properties":{"chapters":{"items":{"properties":{"start":{"type":"string"},"title":{"type":"string"}},"required":["start","title"],"type":"object"},"type":"array"}},"required":["chapters"],"type":"object"}"#
        );
    }
}
