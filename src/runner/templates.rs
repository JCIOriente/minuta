//! The templates of a run, after the pipeline, in dependency order.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::config::Config;
use crate::error::{Error, Result};
use crate::generate::{self, TemplateRun};
use crate::job::{layout, template_reason, Ctx, Job};
use crate::llm::call::{CallCache, Policy};
use crate::llm::{Backend, BackendId};
use crate::progress::{template_detail, Progress};
use crate::transcript::Transcript;

use super::pipeline::interrupted_by;
use super::Resolved;

/// The templates of a run, after the pipeline. Kept apart from [`Pipeline`]: templates are
/// recorded under `templates` in `job.json`, and their fingerprints can only be computed
/// once their input exists.
pub(super) struct TemplatePhase<'a> {
    pub(super) ctx: &'a Ctx,
    pub(super) job_name: &'a str,
    pub(super) resolved: &'a Resolved,
    pub(super) backends: &'a BTreeMap<String, Box<dyn Backend>>,
    pub(super) config: &'a Config,
    pub(super) interrupt: &'a Arc<AtomicBool>,
    /// The progress index of the first template.
    pub(super) first_index: usize,
}

impl TemplatePhase<'_> {
    /// Runs the stale templates in dependency order, marking each done as it finishes;
    /// returns how many ran. The first failure stops the run.
    pub(super) fn run(&self, job: &mut Job, progress: &mut Progress) -> Result<usize> {
        if self.resolved.order.is_empty() {
            return Ok(0);
        }
        CallCache::new(&self.ctx.dir).clean_tmp()?;
        let detected = detected_language(&self.ctx.dir)?;
        let implicit = self.implicit_templates(job);
        let mut ran = 0;
        for (i, name) in self.resolved.order.iter().enumerate() {
            let index = self.first_index + i;
            if self.interrupt.load(Ordering::SeqCst) {
                return Err(Error::Interrupted);
            }
            match self.run_one(job, name, index, &detected, &implicit, progress) {
                Ok(true) => ran += 1,
                Ok(false) => {}
                Err(e) => {
                    let interrupted = self.interrupt.load(Ordering::SeqCst);
                    let e = if interrupted || interrupted_by(&e, self.interrupt) {
                        Error::Interrupted
                    } else {
                        e
                    };
                    progress.failed(index, name, matches!(e, Error::Interrupted));
                    return Err(e);
                }
            }
        }
        if self.interrupt.load(Ordering::SeqCst) {
            return Err(Error::Interrupted);
        }
        Ok(ran)
    }

    /// The templates that run only as a dependency, whose `.md` goes to `assets/`. One that
    /// an earlier run wrote to the job folder (it was requested then) stays there.
    fn implicit_templates(&self, job: &Job) -> Vec<String> {
        let kept_in_place = |name: &String| {
            let recorded = job.state.templates.get(name);
            // The `.md` is the last artifact of every kind of template.
            let md = recorded.and_then(|r| r.artifacts.last());
            md.is_some_and(|artifact| !artifact.starts_with(layout::ASSETS_DIR))
        };
        let resolved = self.resolved;
        let is_implicit =
            |name: &&String| !resolved.requested.contains(name) && !kept_in_place(name);
        resolved.order.iter().filter(is_implicit).cloned().collect()
    }

    /// Runs template `name` when it is stale; returns whether it ran. `implicit` lists the
    /// implicit templates of the run.
    fn run_one(
        &self,
        job: &mut Job,
        name: &str,
        index: usize,
        detected: &str,
        implicit: &[String],
        progress: &mut Progress,
    ) -> Result<bool> {
        let resolved = self.resolved;
        let template = resolved.catalog.get(name)?;
        let id = resolved.backends[name].to_string();
        let backend = self.backends[&id].as_ref();
        let output_lang = generate::output_lang(
            name,
            resolved.flag_lang.as_deref(),
            template.lang.as_deref(),
            resolved.config_lang.as_deref(),
            detected,
        )?;
        let forced = resolved.forced.iter().any(|f| f == name);
        let run = TemplateRun {
            template,
            backend,
            output_lang,
            forced,
            repairs: self.config.repairs(),
            implicit: implicit.iter().any(|n| n == name),
            implicit_inputs: implicit,
        };
        let fingerprint = run.fingerprint(self.ctx, self.job_name)?;
        let Some(reason) = template_reason(job, name, &fingerprint, forced)? else {
            if run.move_implicit(self.ctx)? {
                let seconds = job.state.templates.get(name).and_then(|t| t.seconds);
                let artifacts = run.artifacts(&self.ctx.dir);
                job.mark_template_done(name, &fingerprint, seconds.unwrap_or(0.0), &artifacts)?;
            }
            progress.cached(index, name);
            return Ok(false);
        };
        job.mark_template_stale(name, &reason)?;
        progress.start_template(index, name);
        let started = Instant::now();
        let policy = Policy {
            retries: self.config.retries(),
            backoff: self.config.backoff(),
        };
        let concurrency = concurrency(self.config, &resolved.backends[name]);
        let generated = run.execute(
            self.ctx,
            self.job_name,
            &policy,
            concurrency,
            self.interrupt,
            &mut |event| progress.event(event),
        )?;
        let seconds = started.elapsed().as_secs_f64();
        job.mark_template_done(name, &fingerprint, seconds, &run.artifacts(&self.ctx.dir))?;
        let detail = template_detail(&id, generated.calls);
        progress.done(index, name, Some(&detail), seconds);
        Ok(true)
    }
}

/// Parallel calls within a template: `llm.concurrency`, or 1 for Ollama and endpoints (one
/// local model serves one request at a time) and 3 for Claude and Codex.
fn concurrency(config: &Config, id: &BackendId) -> usize {
    config.concurrency().unwrap_or(match id {
        BackendId::Ollama { .. } | BackendId::Endpoint { .. } => 1,
        BackendId::Claude { .. } | BackendId::Codex { .. } => 3,
    })
}

/// The speech language recorded in the job's transcript.
fn detected_language(dir: &Path) -> Result<String> {
    let path = layout::transcript_json(dir);
    let json = fs::read_to_string(&path)
        .map_err(|e| Error::Io(io::Error::new(e.kind(), format!("{}: {e}", path.display()))))?;
    let transcript = Transcript::from_json(&json)
        .map_err(|e| Error::Parse(format!("{}: {e}", path.display())))?;
    Ok(transcript.language)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::test_support::config;

    #[test]
    fn concurrency_defaults_per_backend() {
        let ollama = BackendId::Ollama {
            model: "gemma4:26b".into(),
        };
        let claude = BackendId::Claude { model: None };
        assert_eq!(concurrency(&config(&[]), &ollama), 1);
        assert_eq!(concurrency(&config(&[]), &claude), 3);
        let set = config(&[("MINUTA_LLM_CONCURRENCY", "5")]);
        assert_eq!(concurrency(&set, &ollama), 5);
        assert_eq!(concurrency(&set, &claude), 5);
    }

    #[test]
    fn endpoint_concurrency_defaults_to_one() {
        let id = BackendId::Endpoint {
            name: "box".into(),
            model: "m".into(),
        };
        assert_eq!(concurrency(&config(&[]), &id), 1);
        assert_eq!(
            concurrency(&config(&[("MINUTA_LLM_CONCURRENCY", "4")]), &id),
            4
        );
    }
}
