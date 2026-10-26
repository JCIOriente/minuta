//! Chapter rendering: the chapters artifact and its text forms (ADR-0009).

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use super::{lines, Chapter};

/// The version of the `chapters.json` layout this build writes and reads (ADR-0009).
const ARTIFACT_VERSION: u64 = 1;

/// A chapters artifact as read back from `chapters.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub output_lang: String,
    pub chapters: Vec<Chapter>,
}

/// The `chapters.md` text: the chapter lines, one trailing newline.
pub fn markdown(chapters: &[Chapter]) -> String {
    format!("{}\n", lines(chapters))
}

/// The `chapters.json` artifact (ADR-0009), pretty-printed with a trailing newline.
pub fn artifact_json(output_lang_code: &str, chapters: &[Chapter]) -> String {
    let artifact = ArtifactJson {
        version: ARTIFACT_VERSION,
        output_lang: output_lang_code,
        chapters,
    };
    let mut json = serde_json::to_string_pretty(&artifact).expect("artifact serializes");
    json.push('\n');
    json
}

#[derive(Serialize)]
struct ArtifactJson<'a> {
    version: u64,
    output_lang: &'a str,
    chapters: &'a [Chapter],
}

/// The shape of `chapters.json`, checked after its version.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    // Checked on the untyped value before this struct is read, so it is never read here.
    #[allow(dead_code)]
    version: u64,
    output_lang: String,
    chapters: Vec<WireChapter>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireChapter {
    start: u64,
    title: String,
}

/// Reads a `chapters.json` text. Errors are the spec's wording after "chapters.json".
pub fn read_artifact(text: &str) -> Result<Artifact, String> {
    let value: Json = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let Some(object) = value.as_object() else {
        return Err("expected an object".to_string());
    };
    if let Some(version) = object.get("version").and_then(Json::as_u64) {
        if version != ARTIFACT_VERSION {
            return Err(format!(
                "has version {version}, which this version of minuta cannot read"
            ));
        }
    }
    // Serde's derived structs also read arrays, so each chapter must be an object here.
    if let Some(Json::Array(chapters)) = object.get("chapters") {
        if let Some(i) = chapters.iter().position(|c| !c.is_object()) {
            return Err(format!("chapter {} is not an object", i + 1));
        }
    }
    let envelope: Envelope = serde_json::from_value(value).map_err(|e| e.to_string())?;
    Ok(Artifact {
        output_lang: envelope.output_lang,
        chapters: envelope
            .chapters
            .into_iter()
            .map(|c| Chapter {
                start: c.start,
                title: c.title,
            })
            .collect(),
    })
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
            read_artifact(
                r#"{"version":1,"output_lang":"es","chapters":[{"start":"4:12","title":"Intro"}]}"#
            ),
            Err("invalid type: string \"4:12\", expected u64".to_string())
        );
    }

    #[test]
    fn read_artifact_rejects_unknown_chapter_key() {
        assert_eq!(
            read_artifact(
                r#"{"version":1,"output_lang":"es","chapters":[{"start":0,"title":"Intro","end":5}]}"#
            ),
            Err("unknown field `end`, expected `start` or `title`".to_string())
        );
    }

    #[test]
    fn read_artifact_rejects_non_object() {
        assert_eq!(read_artifact("[]"), Err("expected an object".to_string()));
    }

    #[test]
    fn read_artifact_rejects_array_envelope() {
        assert_eq!(
            read_artifact(r#"[1,"es",[[0,"Intro"]]]"#),
            Err("expected an object".to_string())
        );
    }

    #[test]
    fn read_artifact_rejects_array_chapter() {
        assert_eq!(
            read_artifact(r#"{"version":1,"output_lang":"es","chapters":[[0,"Intro"]]}"#),
            Err("chapter 1 is not an object".to_string())
        );
    }

    #[test]
    fn read_artifact_rejects_unknown_top_level_key() {
        assert_eq!(
            read_artifact(r#"{"version":1,"output_lang":"es","chapters":[],"extra":1}"#),
            Err(
                "unknown field `extra`, expected one of `version`, `output_lang`, `chapters`"
                    .to_string()
            )
        );
    }

    #[test]
    fn read_artifact_rejects_missing_version() {
        assert_eq!(
            read_artifact(r#"{"output_lang":"es","chapters":[]}"#),
            Err("missing field `version`".to_string())
        );
    }

    #[test]
    fn read_artifact_rejects_version_0() {
        assert_eq!(
            read_artifact(r#"{"version":0,"output_lang":"es","chapters":[]}"#),
            Err("has version 0, which this version of minuta cannot read".to_string())
        );
    }
}
