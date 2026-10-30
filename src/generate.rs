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

use crate::chapters::render::Artifact;
use crate::chapters::{
    self, CHAPTERS_VERSION, JSON_FILE, MIN_CHAPTERS, MIN_SECONDS, MIN_TRANSCRIPT_SECONDS,
};
use crate::error::{Error, Result};
use crate::fields::prompt::{fields_block, repair_system, repair_user, schema};
use crate::fields::render::{artifact_json, render};
use crate::fields::validate::parse_answer;
use crate::fields::{Violation, FIELDS_VERSION};
use crate::job::{fingerprint, layout, write_atomic_in, Ctx};
use crate::lang;
use crate::llm::call::{call, call_json, Answer, CallCache, Policy};
use crate::llm::chunk::{self, transcript_text, Event, Generated, Request, PROMPT_VERSION};
use crate::llm::{Backend, BackendId};
use crate::steps::transcribe::TRANSCRIPT_FILE;
use crate::template::{Input, Output, Template, Vars};
use crate::transcript::Transcript;

/// What to try when a JSON answer still breaks its limits or rules after every repair call.
const REPAIRS_HINT: &str = "raise llm.repairs, relax the limit in a project template, \
                            or use a stronger backend with --llm";

/// A plain-text call through the call cache, as `chunk::generate_with` takes it.
type Complete<'a> = &'a (dyn Fn(&str, &str) -> Result<String> + Sync);

/// What the shared repair loop needs from a JSON output (fields or chapters).
struct JsonOutput<'a, T> {
    /// Goes before the language instruction in the final system prompt.
    block: String,
    /// The system prompt of every repair call.
    repair_system: String,
    schema: serde_json::Value,
    /// Parses and checks a cleaned answer.
    accept: &'a dyn Fn(&str) -> std::result::Result<T, Vec<Violation>>,
    /// What a violation breaks, as the exhausted message words it: "limit" or "rule".
    broken: &'static str,
}

/// What every LLM call of one `execute` shares.
struct Calls<'a> {
    cache: CallCache,
    policy: &'a Policy,
    interrupt: &'a AtomicBool,
}

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
    /// Repair calls after a rejected fields or chapters answer (`llm.repairs`); unused for
    /// Markdown.
    pub repairs: u32,
    /// The template runs only because another one needs it: its files go to
    /// `.minuta/derived/` instead of the job folder and `assets/`.
    pub implicit: bool,
    /// The templates of this run that are implicit. A template that reads one of them (as its
    /// input or as the chapters it fills in) finds its files in `.minuta/derived/`.
    pub implicit_inputs: &'a [String],
}

impl TemplateRun<'_> {
    /// The artifacts the template writes in the job folder `dir`, in write order: `<name>.md`,
    /// or `<name>.json` then `<name>.md` for a fields template (ADR-0007), or `chapters.json`
    /// then `<name>.md` for the chapters template (ADR-0009). The `.md` is in `dir` and the
    /// `.json` in `assets/`; an implicit template keeps both in `.minuta/derived/`.
    pub fn artifacts(&self, dir: &Path) -> Vec<PathBuf> {
        self.artifacts_at(dir, self.implicit)
    }

    fn artifacts_at(&self, dir: &Path, implicit: bool) -> Vec<PathBuf> {
        let md = md_path(dir, &self.template.name, implicit);
        match self.template.output {
            Output::Markdown => vec![md],
            Output::Fields(_) => vec![json_path(dir, &self.template.name, implicit), md],
            Output::Chapters(_) => vec![json_path(dir, chapters::NAME, implicit), md],
        }
    }

    /// A requested template whose files an earlier run left in `.minuta/derived/` (it ran as
    /// an implicit dependency then) gets them moved to their normal places, so an unchanged
    /// template needs no new run. Returns whether anything moved. Does nothing for an
    /// implicit template.
    pub fn move_derived(&self, ctx: &Ctx) -> Result<bool> {
        if self.implicit {
            return Ok(false);
        }
        let mut moved = false;
        let pairs = self
            .artifacts_at(&ctx.dir, true)
            .into_iter()
            .zip(self.artifacts_at(&ctx.dir, false));
        for (derived, normal) in pairs {
            if !exists(&derived)? || exists(&normal)? {
                continue;
            }
            let bytes = fs::read(&derived).map_err(|e| io_error(&derived, e))?;
            write_atomic_in(&ctx.dir, &normal, |tmp| {
                fs::write(tmp, &bytes)?;
                Ok(())
            })?;
            fs::remove_file(&derived).map_err(|e| io_error(&derived, e))?;
            moved = true;
        }
        Ok(moved)
    }

    /// The template's fingerprint. It hashes the content of the input artifact and the
    /// rendered variable values (which come from `transcript.json` even for a template input),
    /// so call it only once the input exists, never ahead of time for a chain of templates.
    /// A fields template adds `FIELDS_VERSION`, then, when it declares `chapters`, the content
    /// hash of `chapters.json` and `CHAPTERS_VERSION`. The chapters template adds
    /// `CHAPTERS_VERSION`. Markdown templates keep their parts.
    pub fn fingerprint(&self, ctx: &Ctx, job_name: &str) -> Result<String> {
        let input = self.read_input(ctx)?;
        let vars = self.vars(&read_transcript(&ctx.dir)?, job_name)?;
        let t = self.template;
        let chapters_digest = match &t.output {
            Output::Fields(spec) if spec.declares_chapters() => {
                Some(Sha256::digest(self.read_chapters_json(ctx)?))
            }
            _ => None,
        };
        let input_digest = Sha256::digest(&input);
        let prompt_version = PROMPT_VERSION.to_string();
        let context_tokens = self.backend.context_tokens().to_string();
        let fields_version = FIELDS_VERSION.to_string();
        let chapters_version = CHAPTERS_VERSION.to_string();
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
        match &t.output {
            Output::Markdown => {}
            Output::Fields(_) => {
                parts.push(fields_version.as_bytes());
                if let Some(digest) = &chapters_digest {
                    parts.push(digest);
                    parts.push(chapters_version.as_bytes());
                }
            }
            Output::Chapters(_) => parts.push(chapters_version.as_bytes()),
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

        // The chapters a fields template fills in, read once and checked before any call.
        let filled = match &self.template.output {
            Output::Fields(spec) if spec.declares_chapters() => Some(self.read_chapters(ctx)?),
            _ => None,
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
        let calls = Calls {
            cache: CallCache::new(&ctx.dir),
            policy,
            interrupt,
        };
        let complete = |system: &str, user: &str| {
            call(
                self.backend,
                &calls.cache,
                calls.policy,
                system,
                user,
                self.forced,
                calls.interrupt,
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
                let accept = |text: &str| parse_answer(text, spec);
                let output = JsonOutput {
                    block: fields_block(spec),
                    repair_system: repair_system(spec, request.output_lang),
                    schema: schema(spec),
                    accept: &accept,
                    broken: "limit",
                };
                let (generated, values) = self
                    .generate_json(&request, &complete, &calls, &output, on_event)
                    .map_err(named)?;
                let values = match filled {
                    Some(artifact) => values.fill_chapters(spec, artifact.chapters),
                    None => values,
                };
                let json = artifact_json(&self.template.name, &self.output_lang, &values);
                (generated, vec![json, render(&values, spec.format)])
            }
            Output::Chapters(spec) => {
                let end = end_seconds(&transcript);
                if end < MIN_TRANSCRIPT_SECONDS {
                    return Err(self.error(format!(
                        "the transcript ends at {}; YouTube chapters need at least \
                         {MIN_TRANSCRIPT_SECONDS} s ({MIN_CHAPTERS} chapters of at least \
                         {MIN_SECONDS} s). To run a template that declares the chapters field \
                         on it, override it in ./templates without that field.",
                        chapters::stamp(end)
                    )));
                }
                let accept = |text: &str| {
                    chapters::validate::parse_answer(text, &transcript.segments, end, spec)
                };
                let output = JsonOutput {
                    block: chapters::prompt::chapters_block(spec),
                    repair_system: chapters::prompt::repair_system(spec, request.output_lang),
                    schema: chapters::prompt::schema(),
                    accept: &accept,
                    broken: "rule",
                };
                let (generated, list) = self
                    .generate_json(&request, &complete, &calls, &output, on_event)
                    .map_err(named)?;
                let json = chapters::render::artifact_json(&self.output_lang, &list);
                (generated, vec![json, chapters::render::markdown(&list)])
            }
        };

        let mut written: Vec<PathBuf> = Vec::new();
        for (path, text) in self.artifacts(&ctx.dir).into_iter().zip(&contents) {
            let result = write_atomic_in(&ctx.dir, &path, |tmp| {
                fs::write(tmp, text)?;
                Ok(())
            });
            if let Err(e) = result {
                return Err(remove_written(&written, e));
            }
            written.push(path);
        }
        self.remove_derived(ctx)?;
        Ok(generated)
    }

    /// A requested template supersedes the copies it left in `.minuta/derived/` while it was
    /// an implicit dependency.
    fn remove_derived(&self, ctx: &Ctx) -> Result<()> {
        if self.implicit {
            return Ok(());
        }
        for path in self.artifacts_at(&ctx.dir, true) {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(io_error(&path, e)),
            }
        }
        Ok(())
    }

    /// Generates a JSON output (fields or chapters): the final call asks for JSON with
    /// `output.schema`, and a rejected answer gets up to `repairs` repair calls, each built from
    /// the previous answer and its violations (ADR-0006). Map calls use `complete` as for
    /// Markdown.
    fn generate_json<T>(
        &self,
        request: &Request,
        complete: Complete,
        calls: &Calls,
        output: &JsonOutput<T>,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<(Generated, T)> {
        let accept = |text: &str| (output.accept)(text).map(|value| (value, text.to_owned()));
        let ask = |system: &str, user: &str| {
            call_json(
                self.backend,
                &calls.cache,
                calls.policy,
                system,
                user,
                &output.schema,
                self.forced,
                calls.interrupt,
                &accept,
            )
        };
        let mut accepted = None;
        let mut repair_calls = 0;
        let mut finish = |system: &str, user: &str, on_event: &mut dyn FnMut(Event)| {
            let mut answer = ask(system, user)?;
            loop {
                let (text, violations) = match answer {
                    Answer::Accepted((value, text)) => {
                        accepted = Some(value);
                        return Ok(text);
                    }
                    Answer::Rejected { text, violations } => (text, violations),
                };
                if repair_calls == self.repairs {
                    return Err(self.repairs_exhausted(&violations, output.broken));
                }
                repair_calls += 1;
                on_event(Event::Repair {
                    attempt: repair_calls as usize,
                    total: self.repairs as usize,
                });
                answer = ask(&output.repair_system, &repair_user(&text, &violations))?;
            }
        };
        let mut generated = chunk::generate_with(
            request,
            Some(&output.block),
            complete,
            &mut finish,
            on_event,
        )?;
        // `generate_with` counts the final call once; add the repair calls.
        generated.calls += repair_calls as usize;
        let value = accepted.expect("finish returned Ok only after accepting an answer");
        Ok((generated, value))
    }

    /// `broken` is what one violation breaks: "limit" or "rule".
    fn repairs_exhausted(&self, violations: &[Violation], broken: &str) -> Error {
        let count = violations.len();
        let broken = if count == 1 {
            broken.to_owned()
        } else {
            format!("{broken}s")
        };
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
            "the answer still breaks {count} {broken} after {} {repairs}: {}; {REPAIRS_HINT}",
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
        let path = match &self.template.input {
            Input::Transcript => ctx.dir.join(TRANSCRIPT_FILE),
            Input::Template(dep) => md_path(&ctx.dir, dep, self.is_implicit_input(dep)),
        };
        self.read_job_file(&path, &self.input_file())
    }

    fn is_implicit_input(&self, name: &str) -> bool {
        self.implicit_inputs.iter().any(|n| n == name)
    }

    /// The file at `path`, called `file` in messages; a missing file is a template error, any
    /// other I/O error names the path.
    fn read_job_file(&self, path: &Path, file: &str) -> Result<Vec<u8>> {
        fs::read(path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => self.error(format!("input {file} is missing")),
            _ => io_error(path, e),
        })
    }

    fn read_chapters_json(&self, ctx: &Ctx) -> Result<Vec<u8>> {
        let path = json_path(
            &ctx.dir,
            chapters::NAME,
            self.is_implicit_input(chapters::NAME),
        );
        self.read_job_file(&path, JSON_FILE)
    }

    /// The chapters a fields template fills in: `chapters.json`, read and checked against the
    /// output language (ADR-0009).
    fn read_chapters(&self, ctx: &Ctx) -> Result<Artifact> {
        let bytes = self.read_chapters_json(ctx)?;
        let damaged = |message: String| self.error(format!("{JSON_FILE}: {message}"));
        let text = std::str::from_utf8(&bytes).map_err(|e| damaged(e.to_string()))?;
        let artifact = chapters::render::read_artifact(text).map_err(|message| {
            // `read_artifact` words an unknown version as a sentence after the file name.
            if message.starts_with("has version ") {
                self.error(format!("{JSON_FILE} {message}"))
            } else {
                damaged(message)
            }
        })?;
        if artifact.output_lang != self.output_lang {
            return Err(self.error(format!(
                "{JSON_FILE} is in {} but {} writes {}; give both templates the same lang, \
                 or use --output-lang",
                artifact.output_lang, self.template.name, self.output_lang
            )));
        }
        Ok(artifact)
    }
}

/// `<name>.md`: in the job folder, or in `.minuta/derived/` for an implicit template.
fn md_path(dir: &Path, name: &str, implicit: bool) -> PathBuf {
    let home = if implicit {
        layout::derived_dir(dir)
    } else {
        dir.to_path_buf()
    };
    home.join(format!("{name}.md"))
}

/// `<name>.json`: in `assets/`, or in `.minuta/derived/` for an implicit template.
fn json_path(dir: &Path, name: &str, implicit: bool) -> PathBuf {
    if implicit {
        layout::derived_dir(dir).join(format!("{name}.json"))
    } else {
        layout::template_json(dir, name)
    }
}

fn exists(path: &Path) -> Result<bool> {
    path.try_exists().map_err(|e| io_error(path, e))
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

/// The end of the last segment in whole seconds: the transcript end the chapter rules use.
fn end_seconds(t: &Transcript) -> u64 {
    let end = t.segments.last().map_or(0.0, |s| s.end);
    // Truncation to whole seconds is intended; negative ends clamp to zero.
    end.max(0.0) as u64
}

/// `h:mm:ss` at the end of the last segment, truncated to whole seconds.
fn duration(t: &Transcript) -> String {
    let secs = end_seconds(t);
    format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}
