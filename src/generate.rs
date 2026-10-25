//! The template step: one template run against a job (spec "Template step and job state").
//!
//! The runner resolves the output language and backend with [`output_lang`] and
//! [`backend_id`], then, once the template's input exists, asks [`TemplateRun::fingerprint`]
//! whether it is current and runs [`TemplateRun::execute`] if not (ADR-0005).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::job::{fingerprint, write_atomic, Ctx};
use crate::lang;
use crate::llm::call::{call, CallCache, Policy};
use crate::llm::chunk::{self, transcript_text, Event, Generated, Request, PROMPT_VERSION};
use crate::llm::{Backend, BackendId};
use crate::steps::transcribe::TRANSCRIPT_FILE;
use crate::template::{Input, Template, Vars};
use crate::transcript::Transcript;

/// The output language code for template `name`: `flag` > `template` > `config` > `detected`
/// (the speech language in `transcript.json`). A chosen flag, template or config code that is
/// unknown is a usage error. An unknown detected code with no other source fails the template
/// with a hint to pass `--output-lang`.
pub fn output_lang(
    name: &str,
    flag: Option<&str>,
    template: Option<&str>,
    config: Option<&str>,
    detected: &str,
) -> Result<String> {
    if let Some(code) = flag.or(template).or(config) {
        lang::validate(code)?;
        return Ok(code.to_owned());
    }
    if lang::name(detected).is_none() {
        return Err(Error::Template {
            name: name.to_owned(),
            message: format!(
                "the transcript's language '{detected}' is not a known language code; \
                 choose the output language with --output-lang"
            ),
        });
    }
    Ok(detected.to_owned())
}

/// The backend a template runs on: `flag` > `template` > `default`.
pub fn backend_id(
    flag: Option<&BackendId>,
    template: Option<&BackendId>,
    default: &BackendId,
) -> BackendId {
    flag.or(template).unwrap_or(default).clone()
}

/// One template, resolved for a run.
pub struct TemplateRun<'a> {
    pub template: &'a Template,
    pub backend: &'a dyn Backend,
    /// Output language code.
    pub output_lang: String,
    /// The template's stale reason is `forced`: its calls skip the cache read.
    pub forced: bool,
}

impl TemplateRun<'_> {
    /// The artifact the template writes, relative to the job folder: `<name>.md`.
    pub fn artifact(&self) -> PathBuf {
        PathBuf::from(format!("{}.md", self.template.name))
    }

    /// The template's fingerprint. It hashes the content of the input artifact and the
    /// rendered variable values (which come from `transcript.json` even for a template input),
    /// so call it only once the input exists, never ahead of time for a chain of templates.
    pub fn fingerprint(&self, ctx: &Ctx, job_name: &str) -> Result<String> {
        let input = self.read_input(ctx)?;
        let vars = self.vars(&read_transcript(&ctx.dir)?, job_name)?;
        let t = self.template;
        Ok(fingerprint(&[
            b"template",
            t.name.as_bytes(),
            t.raw.as_bytes(),
            PROMPT_VERSION.to_string().as_bytes(),
            self.backend.id().as_bytes(),
            self.backend.context_tokens().to_string().as_bytes(),
            self.output_lang.as_bytes(),
            &Sha256::digest(&input),
            vars.output_lang.as_bytes(),
            vars.output_lang_code.as_bytes(),
            vars.speech_lang.as_bytes(),
            vars.job_name.as_bytes(),
            vars.duration.as_bytes(),
        ]))
    }

    /// Runs the template and writes `<name>.md` atomically. Nothing is written on failure.
    pub fn execute(
        &self,
        ctx: &Ctx,
        job_name: &str,
        policy: &Policy,
        concurrency: usize,
        interrupt: &AtomicBool,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<Generated> {
        let transcript = read_transcript(&ctx.dir)?;
        let input = match &self.template.input {
            Input::Transcript => {
                // Zero segments, or only blank ones: there is nothing to send.
                let text = transcript_text(&transcript);
                if text.trim().is_empty() {
                    return Err(self.error("transcript is empty".into()));
                }
                text
            }
            Input::Template(_) => {
                let bytes = self.read_input(ctx)?;
                String::from_utf8(bytes).map_err(|_| {
                    self.error(format!("input {} is not valid UTF-8", self.input_file()))
                })?
            }
        };

        let vars = self.vars(&transcript, job_name)?;
        let body = self.template.render(&vars);
        let request = Request {
            body: &body,
            output_lang: &vars.output_lang,
            input: &input,
            context_tokens: self.backend.context_tokens(),
            concurrency,
        };
        let cache = CallCache::new(&ctx.dir);
        let complete = |system: &str, user: &str| {
            call(
                self.backend,
                &cache,
                policy,
                system,
                user,
                self.forced,
                interrupt,
            )
        };
        let generated = chunk::generate(&request, &complete, on_event).map_err(|e| match e {
            Error::Template { message, .. } => self.error(message),
            other => other,
        })?;

        let mut text = generated.text.clone();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        write_atomic(&ctx.dir.join(self.artifact()), |tmp| {
            fs::write(tmp, &text)?;
            Ok(())
        })?;
        Ok(generated)
    }

    fn vars(&self, transcript: &Transcript, job_name: &str) -> Result<Vars> {
        Ok(Vars {
            output_lang: lang::validate(&self.output_lang)?.to_owned(),
            output_lang_code: self.output_lang.clone(),
            speech_lang: lang::name(&transcript.language)
                .unwrap_or(&transcript.language)
                .to_owned(),
            job_name: job_name.to_owned(),
            duration: duration(transcript),
        })
    }

    fn error(&self, message: String) -> Error {
        Error::Template {
            name: self.template.name.clone(),
            message,
        }
    }

    /// The input artifact's file name in the job folder.
    fn input_file(&self) -> String {
        match &self.template.input {
            Input::Transcript => TRANSCRIPT_FILE.to_owned(),
            Input::Template(dep) => format!("{dep}.md"),
        }
    }

    fn read_input(&self, ctx: &Ctx) -> Result<Vec<u8>> {
        let file = self.input_file();
        let path = ctx.dir.join(&file);
        fs::read(&path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => self.error(format!("input {file} is missing")),
            _ => io_error(&path, e),
        })
    }
}

fn read_transcript(dir: &Path) -> Result<Transcript> {
    let path = dir.join(TRANSCRIPT_FILE);
    let json = fs::read_to_string(&path).map_err(|e| io_error(&path, e))?;
    serde_json::from_str(&json).map_err(|e| Error::Parse(format!("{TRANSCRIPT_FILE}: {e}")))
}

fn io_error(path: &Path, e: io::Error) -> Error {
    Error::Io(io::Error::new(e.kind(), format!("{}: {e}", path.display())))
}

/// `h:mm:ss` at the end of the last segment, truncated to whole seconds.
fn duration(t: &Transcript) -> String {
    let end = t.segments.last().map_or(0.0, |s| s.end);
    // Truncation to whole seconds is intended; negative ends clamp to zero.
    let secs = end.max(0.0) as u64;
    format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}
