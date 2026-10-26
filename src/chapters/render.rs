//! Chapter rendering: the chapters artifact and its text forms (ADR-0009).

use super::Chapter;

/// A chapters artifact as read back from `chapters.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub output_lang: String,
    pub chapters: Vec<Chapter>,
}

/// The `chapters.md` text: the chapter lines, one trailing newline.
pub fn markdown(chapters: &[Chapter]) -> String {
    todo!("markdown {chapters:?}")
}

/// The `chapters.json` artifact (ADR-0009), pretty-printed with a trailing newline.
pub fn artifact_json(output_lang_code: &str, chapters: &[Chapter]) -> String {
    todo!("artifact_json {output_lang_code} {chapters:?}")
}

/// Reads a `chapters.json` text. Errors are the spec's wording after "chapters.json".
pub fn read_artifact(text: &str) -> Result<Artifact, String> {
    todo!("read_artifact {text}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_chapters() -> Vec<Chapter> {
        vec![
            Chapter {
                start: 0,
                title: "Intro".into(),
            },
            Chapter {
                start: 252,
                title: "Ownership".into(),
            },
        ]
    }

    #[test]
    fn markdown_trailing_newline() {
        assert_eq!(markdown(&two_chapters()), "0:00 Intro\n4:12 Ownership\n");
    }

    #[test]
    fn artifact_json_golden() {
        let expected = "{\n  \"version\": 1,\n  \"output_lang\": \"es\",\n  \"chapters\": [\n    {\n      \"start\": 0,\n      \"title\": \"Intro\"\n    },\n    {\n      \"start\": 252,\n      \"title\": \"Ownership\"\n    }\n  ]\n}\n";
        assert_eq!(artifact_json("es", &two_chapters()), expected);
    }

    #[test]
    fn read_artifact_round_trip() {
        assert_eq!(
            read_artifact(&artifact_json("es", &two_chapters())),
            Ok(Artifact {
                output_lang: "es".into(),
                chapters: two_chapters(),
            })
        );
    }

    #[test]
    fn read_artifact_rejects_version_2() {
        assert_eq!(
            read_artifact(r#"{"version":2,"output_lang":"es","chapters":[]}"#),
            Err("has version 2, which this version of minuta cannot read".to_string())
        );
    }

    #[test]
    fn read_artifact_rejects_missing_chapters() {
        assert_eq!(
            read_artifact(r#"{"version":1,"output_lang":"es"}"#),
            Err("missing field `chapters`".to_string())
        );
    }

    #[test]
    fn read_artifact_rejects_string_start() {
        assert_eq!(
            read_artifact(r#"{"version":1,"output_lang":"es","chapters":[{"start":"4:12","title":"Intro"}]}"#),
            Err("invalid type: string \"4:12\", expected u64".to_string())
        );
    }

    #[test]
    fn read_artifact_rejects_non_object() {
        assert_eq!(
            read_artifact("[]"),
            Err("invalid type: sequence, expected struct Envelope".to_string())
        );
    }
}
