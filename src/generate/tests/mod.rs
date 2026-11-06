use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::chapters::prompt as chapters_prompt;
use crate::chapters::{ChaptersSpec, CHAPTERS_VERSION};
use crate::error::Error;
use crate::fields::prompt::{fields_block, repair_system, repair_user, schema};
use crate::fields::{FieldsSpec, Violation, FIELDS_VERSION};
use crate::generate::{backend_id, output_lang, TemplateRun};
use crate::job::layout::{implicit_md, template_json};
use crate::job::{fingerprint, Ctx};
use crate::llm::call::Policy;
use crate::llm::chunk::{language_instruction, transcript_text, Event, PROMPT_VERSION};
use crate::llm::fake::ScriptedBackend;
use crate::llm::{Backend, BackendId};
use crate::steps::transcribe::TRANSCRIPT_FILE;
use crate::template::{Layer, Output, Template};
use crate::transcript::{from_whisper_json, Segment, Transcript};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

mod chapters;
mod fields;
mod fields_chapters;
mod fingerprints;
mod markdown;
mod outputs;
mod precedence;

const ID: &str = "ollama:fake";
const CONTEXT: usize = 16384;

fn tpl(name: &str, text: &str) -> Template {
    Template::parse(name, Layer::Project, None, text).expect("template parses")
}

fn ctx(dir: &Path) -> Ctx {
    Ctx {
        dir: dir.to_path_buf(),
        sources: Vec::new(),
        source_digest: String::new(),
    }
}

fn transcript(language: &str, segments: &[(f64, f64, &str)]) -> Transcript {
    Transcript {
        language: language.into(),
        segments: segments
            .iter()
            .map(|&(start, end, text)| Segment {
                start,
                end,
                text: text.into(),
            })
            .collect(),
    }
}

fn write_transcript(dir: &Path, t: &Transcript) {
    let path = dir.join(TRANSCRIPT_FILE);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_string(t).unwrap()).unwrap();
}

/// A job folder holding a short Spanish transcript.
fn job() -> (TempDir, Transcript) {
    let dir = TempDir::new().unwrap();
    let t = transcript(
        "es",
        &[(0.0, 12.0, "Hola a todos."), (12.0, 40.5, "Empezamos.")],
    );
    write_transcript(dir.path(), &t);
    (dir, t)
}

fn policy() -> Policy {
    Policy {
        retries: 1,
        backoff: Duration::ZERO,
    }
}

fn run<'a>(template: &'a Template, backend: &'a dyn Backend, lang: &str) -> TemplateRun<'a> {
    TemplateRun {
        template,
        backend,
        output_lang: lang.into(),
        forced: false,
        repairs: 2,
        implicit: false,
        implicit_inputs: &[],
    }
}

fn execute(r: &TemplateRun, dir: &Path) -> crate::error::Result<crate::llm::chunk::Generated> {
    r.execute(
        &ctx(dir),
        "reunion",
        &policy(),
        1,
        &AtomicBool::new(false),
        &mut |_| {},
    )
}

const TEXT: &str = "Summarize {{job_name}}.\n";
const FIELDS: &str = "---\noutput: fields\nfields: [title, body]\nlimits:\n  body: { max_chars: 10 }\n---\nWrite about {{job_name}}.\n";

const VALID: &str = r#"{"title": "Hola", "body": "Corto."}"#;

fn fields_spec(t: &Template) -> &FieldsSpec {
    match &t.output {
        Output::Fields(spec) => spec,
        Output::Markdown | Output::Chapters(_) => panic!("{} is not a fields template", t.name),
    }
}

fn execute_events(
    r: &TemplateRun,
    dir: &Path,
) -> (
    crate::error::Result<crate::llm::chunk::Generated>,
    Vec<Event>,
) {
    let mut events = Vec::new();
    let result = r.execute(
        &ctx(dir),
        "reunion",
        &policy(),
        1,
        &AtomicBool::new(false),
        &mut |e| events.push(e),
    );
    (result, events)
}

const YOUTUBE: &str = "---\noutput: fields\nfields: [title, chapters, body]\nlimits:\n  body: { max_chars: 10 }\n---\nWrite about {{job_name}}.\n";
const CHAPTERS_ARTIFACT: &str = "{\n  \"version\": 1,\n  \"output_lang\": \"es\",\n  \"chapters\": [\n    {\n      \"start\": 0,\n      \"title\": \"Introducción\"\n    },\n    {\n      \"start\": 12,\n      \"title\": \"Empezamos\"\n    }\n  ]\n}\n";
