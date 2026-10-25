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
use crate::fields::prompt::{fields_block, repair_system, repair_user, schema};
use crate::fields::render::{artifact_json, render};
use crate::fields::validate::parse_answer;
use crate::fields::{FieldsSpec, Values, Violation, FIELDS_VERSION};
use crate::job::{fingerprint, write_atomic, Ctx};
use crate::lang;
use crate::llm::call::{call, call_json, Answer, CallCache, Policy};
use crate::llm::chunk::{self, transcript_text, Event, Generated, Request, PROMPT_VERSION};
use crate::llm::{Backend, BackendId};
use crate::steps::transcribe::TRANSCRIPT_FILE;
use crate::template::{Input, Output, Template, Vars};
use crate::transcript::Transcript;

/// What to try when a fields answer still breaks its limits after every repair call.
const REPAIRS_HINT: &str = "raise llm.repairs, relax the limit in a project template, \
                            or use a stronger backend with --llm";

/// One `call_json` with the fields schema: the accepted values and the answer text, or the
/// rejected answer and its violations.
type Ask<'a> = &'a dyn Fn(&str, &str) -> Result<Answer<(Values, String)>>;

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
    /// Repair calls after a rejected fields answer (`llm.repairs`); unused for Markdown.
    pub repairs: u32,
}

impl TemplateRun<'_> {
    /// The artifacts the template writes, relative to the job folder, in write order:
    /// `<name>.md`, or `<name>.json` then `<name>.md` for a fields template (ADR-0007).
    pub fn artifacts(&self) -> Vec<PathBuf> {
        let file = |ext: &str| PathBuf::from(format!("{}.{ext}", self.template.name));
        match self.template.output {
            Output::Markdown => vec![file("md")],
            Output::Fields(_) => vec![file("json"), file("md")],
        }
    }

    /// The template's fingerprint. It hashes the content of the input artifact and the
    /// rendered variable values (which come from `transcript.json` even for a template input),
    /// so call it only once the input exists, never ahead of time for a chain of templates.
    /// A fields template adds `FIELDS_VERSION`; Markdown templates keep their parts.
    pub fn fingerprint(&self, ctx: &Ctx, job_name: &str) -> Result<String> {
        let input = self.read_input(ctx)?;
        let vars = self.vars(&read_transcript(&ctx.dir)?, job_name)?;
        let t = self.template;
        let input_digest = Sha256::digest(&input);
        let prompt_version = PROMPT_VERSION.to_string();
        let context_tokens = self.backend.context_tokens().to_string();
        let fields_version = FIELDS_VERSION.to_string();
        let mut parts: Vec<&[u8]> = vec![
            b"template",
            t.name.as_bytes(),
            t.raw.as_bytes(),
            prompt_version.as_bytes(),
            self.backend.id().as_bytes(),
            context_tokens.as_bytes(),
            self.output_lang.as_bytes(),
            &input_digest,
            vars.output_lang.as_bytes(),
            vars.output_lang_code.as_bytes(),
            vars.speech_lang.as_bytes(),
            vars.job_name.as_bytes(),
            vars.duration.as_bytes(),
        ];
        if let Output::Fields(_) = t.output {
            parts.push(fields_version.as_bytes());
        }
        Ok(fingerprint(&parts))
    }

    /// Runs the template and writes its [`artifacts`](Self::artifacts) in order, each with
    /// `write_atomic`. A failure before the writes leaves the job folder untouched. If a write
    /// fails, the artifacts this call already wrote are removed (an earlier run's version of
    /// them is not restored), and the write error is returned, naming any file that could not
    /// be removed.
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
        let named = |e| match e {
            Error::Template { message, .. } => self.error(message),
            other => other,
        };
        // One text per artifact, in `artifacts()` order.
        let (generated, contents) = match &self.template.output {
            Output::Markdown => {
                let generated = chunk::generate(&request, &complete, on_event).map_err(named)?;
                let mut text = generated.text.clone();
                if !text.ends_with('\n') {
                    text.push('\n');
                }
                (generated, vec![text])
            }
            Output::Fields(spec) => {
                let schema = schema(spec);
                let accept =
                    |text: &str| parse_answer(text, spec).map(|values| (values, text.to_owned()));
                let ask = |system: &str, user: &str| {
                    call_json(
                        self.backend,
                        &cache,
                        policy,
                        system,
                        user,
                        &schema,
                        self.forced,
                        interrupt,
                        &accept,
                    )
                };
                let (generated, values) = self
                    .generate_fields(spec, &request, &complete, &ask, on_event)
                    .map_err(named)?;
                let json = artifact_json(&self.template.name, &self.output_lang, &values);
                (generated, vec![json, render(&values, spec.format)])
            }
        };

        let mut written: Vec<PathBuf> = Vec::new();
        for (file, text) in self.artifacts().iter().zip(&contents) {
            let path = ctx.dir.join(file);
            let result = write_atomic(&path, |tmp| {
                fs::write(tmp, text)?;
                Ok(())
            });
            if let Err(e) = result {
                return Err(remove_written(&written, e));
            }
            written.push(path);
        }
        Ok(generated)
    }

    /// Generates a fields template: the final call asks for JSON, and a rejected answer gets
    /// up to `repairs` repair calls, each built from the previous answer and its violations
    /// (ADR-0006). Map calls use `complete` as for Markdown.
    fn generate_fields(
        &self,
        spec: &FieldsSpec,
        request: &Request,
        complete: &(dyn Fn(&str, &str) -> Result<String> + Sync),
        ask: Ask,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<(Generated, Values)> {
        let mut accepted = None;
        let mut repair_calls = 0;
        let mut finish = |system: &str, user: &str, on_event: &mut dyn FnMut(Event)| {
            let mut answer = ask(system, user)?;
            loop {
                let (text, violations) = match answer {
                    Answer::Accepted((values, text)) => {
                        accepted = Some(values);
                        return Ok(text);
                    }
                    Answer::Rejected { text, violations } => (text, violations),
                };
                if repair_calls == self.repairs {
                    return Err(self.repairs_exhausted(&violations));
                }
                repair_calls += 1;
                on_event(Event::Repair {
                    attempt: repair_calls as usize,
                    total: self.repairs as usize,
                });
                answer = ask(
                    &repair_system(spec, request.output_lang),
                    &repair_user(&text, &violations),
                )?;
            }
        };
        let block = fields_block(spec);
        let mut generated =
            chunk::generate_with(request, Some(&block), complete, &mut finish, on_event)?;
        // `generate_with` counts the final call once; add the repair calls.
        generated.calls += repair_calls as usize;
        let values = accepted.expect("finish returned Ok only after accepting an answer");
        Ok((generated, values))
    }

    fn repairs_exhausted(&self, violations: &[Violation]) -> Error {
        let count = violations.len();
        let limits = if count == 1 { "limit" } else { "limits" };
        let repairs = if self.repairs == 1 {
            "repair"
        } else {
            "repairs"
        };
        let list: Vec<&str> = violations
            .iter()
            .map(|v| v.0.trim_end_matches('.'))
            .collect();
        self.error(format!(
            "the answer still breaks {count} {limits} after {} {repairs}: {}; {REPAIRS_HINT}",
            self.repairs,
            list.join("; ")
        ))
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

/// Removes `written`, newest first, after `error` stopped the writes, so no partial set of
/// artifacts is left. Returns `error`, with any file that could not be removed added to it.
fn remove_written(written: &[PathBuf], error: Error) -> Error {
    let failures: Vec<String> = written
        .iter()
        .rev()
        .filter_map(|path| {
            fs::remove_file(path)
                .err()
                .map(|e| format!("could not remove {}: {e}", path.display()))
        })
        .collect();
    if failures.is_empty() {
        return error;
    }
    let kind = match &error {
        Error::Io(e) => e.kind(),
        _ => io::ErrorKind::Other,
    };
    Error::Io(io::Error::new(
        kind,
        format!("{error}; {}", failures.join("; ")),
    ))
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
