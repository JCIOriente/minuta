//! The five steps of a run in order, with progress, staleness and interrupts.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::error::{Error, Result};
use crate::job::{layout, step_fingerprint, Ctx, Job, Reason, Step, StepName};
use crate::progress::Progress;
use crate::steps::probe::read_probe;
use crate::steps::{plan_join, Audio, Join, Mode, Outputs, Probe, SourceProbe, Transcribe};
use crate::tools::RunOpts;
use crate::transcript::Transcript;
use serde::de::DeserializeOwned;

/// The progress-aware entry point of a step; returns warnings to show.
trait Execute {
    fn execute(
        &self,
        ctx: &Ctx,
        opts: RunOpts,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<Vec<String>>;

    /// Seconds of media the step's ffmpeg run processes, so its stats can show a percent;
    /// `None` when the step runs no ffmpeg.
    fn media_seconds(&self, _ctx: &Ctx) -> Result<Option<f64>> {
        Ok(None)
    }
}

impl Execute for Probe {
    fn execute(
        &self,
        ctx: &Ctx,
        opts: RunOpts,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<Vec<String>> {
        self.run_with(ctx, opts, on_line).map(|()| Vec::new())
    }
}

impl Execute for Join {
    fn execute(
        &self,
        ctx: &Ctx,
        opts: RunOpts,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<Vec<String>> {
        self.run_with(ctx, opts, on_line).map(|()| Vec::new())
    }

    fn media_seconds(&self, ctx: &Ctx) -> Result<Option<f64>> {
        probed_seconds(ctx)
    }
}

impl Execute for Audio {
    fn execute(
        &self,
        ctx: &Ctx,
        opts: RunOpts,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<Vec<String>> {
        self.run_with(ctx, opts, on_line).map(|()| Vec::new())
    }

    /// The joined file is as long as the sources together.
    fn media_seconds(&self, ctx: &Ctx) -> Result<Option<f64>> {
        probed_seconds(ctx)
    }
}

/// The total length of the sources in `probe.json`; `None` when it is not a positive
/// number of seconds.
fn probed_seconds(ctx: &Ctx) -> Result<Option<f64>> {
    let total: f64 = read_probe(&ctx.dir)?.iter().map(|p| p.duration).sum();
    Ok(Some(total).filter(|t| t.is_finite() && *t > 0.0))
}

impl Execute for Transcribe {
    fn execute(
        &self,
        ctx: &Ctx,
        opts: RunOpts,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<Vec<String>> {
        self.run_with(ctx, opts, on_line)
            .map(|outcome| outcome.warnings)
    }
}

impl Execute for Outputs {
    /// Runs no tool, so there is nothing to stream or kill.
    fn execute(&self, ctx: &Ctx, _: RunOpts, _: &mut dyn FnMut(&str)) -> Result<Vec<String>> {
        self.run(ctx).map(|()| Vec::new())
    }
}

/// A step as the pipeline drives it.
pub(super) trait Runnable {
    fn name(&self) -> StepName;
    fn fingerprint(&self, ctx: &Ctx, previous: Option<&str>) -> String;
    fn artifacts(&self, ctx: &Ctx) -> Vec<PathBuf>;
    fn execute(
        &self,
        ctx: &Ctx,
        opts: RunOpts,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<Vec<String>>;
    fn media_seconds(&self, ctx: &Ctx) -> Result<Option<f64>>;
}

impl<S: Step + Execute> Runnable for S {
    fn name(&self) -> StepName {
        S::NAME
    }

    fn fingerprint(&self, ctx: &Ctx, previous: Option<&str>) -> String {
        step_fingerprint(self, ctx, previous)
    }

    fn artifacts(&self, ctx: &Ctx) -> Vec<PathBuf> {
        self.outputs(ctx)
    }

    fn execute(
        &self,
        ctx: &Ctx,
        opts: RunOpts,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<Vec<String>> {
        Execute::execute(self, ctx, opts, on_line)
    }

    fn media_seconds(&self, ctx: &Ctx) -> Result<Option<f64>> {
        Execute::media_seconds(self, ctx)
    }
}

pub(super) struct Pipeline<'a> {
    pub(super) ctx: &'a Ctx,
    pub(super) plan: &'a [(StepName, Reason)],
    pub(super) interrupt: &'a Arc<AtomicBool>,
}

impl Pipeline<'_> {
    /// Runs the stale steps in order, marking each done as it finishes; returns how many ran.
    pub(super) fn run(
        &self,
        job: &mut Job,
        steps: &[&dyn Runnable],
        current: &[(StepName, String)],
        progress: &mut Progress,
    ) -> Result<usize> {
        let mut ran = 0;
        for (i, (step, (name, fingerprint))) in steps.iter().zip(current).enumerate() {
            let index = i + 1;
            if !self.plan.iter().any(|(stale, _)| stale == name) {
                progress.cached(index, name.as_str());
                continue;
            }
            if self.interrupt.load(Ordering::SeqCst) {
                return Err(Error::Interrupted);
            }
            progress.start(index, name.as_str());
            // The total only turns ffmpeg's stats into a percent. If it can't be read, show
            // the raw stats instead of failing a step that may not need probe.json at all
            // (audio doesn't); a step that does need it reports the error itself.
            progress.set_media_seconds(step.media_seconds(self.ctx).unwrap_or(None));
            let started = Instant::now();
            let opts = RunOpts {
                cwd: None,
                interrupt: Some(Arc::clone(self.interrupt)),
            };
            let warnings = match step.execute(self.ctx, opts, &mut |line| progress.line(line)) {
                Ok(warnings) => warnings,
                Err(e) => {
                    let e = if self.interrupted_by(&e) {
                        Error::Interrupted
                    } else {
                        e
                    };
                    progress.failed(index, name.as_str(), matches!(e, Error::Interrupted));
                    return Err(e);
                }
            };
            let seconds = started.elapsed().as_secs_f64();
            job.mark_done(*name, fingerprint, seconds, &step.artifacts(self.ctx))?;
            // The step is done either way; a detail that can't be read is only a warning.
            let (detail, unreadable) = match detail(*name, self.ctx) {
                Ok(detail) => (detail, None),
                Err(e) => (None, Some(format!("{}: {e}", name.as_str()))),
            };
            progress.done(index, name.as_str(), detail.as_deref(), seconds);
            for warning in unreadable.iter().chain(&warnings) {
                progress.warn(warning);
            }
            ran += 1;
        }
        // Ctrl-C during a step that runs no tool, or while every step was cached.
        if self.interrupt.load(Ordering::SeqCst) {
            return Err(Error::Interrupted);
        }
        Ok(ran)
    }

    fn interrupted_by(&self, err: &Error) -> bool {
        interrupted_by(err, self.interrupt)
    }
}

/// Whether `err` is the result of an interrupt: the tool was killed for it, or died from a
/// signal while the flag is set (the group kill raced the tool's own exit). Tools run in
/// their own process group, so the terminal's Ctrl-C never reaches them: a signal death
/// with the flag clear is a crash or an outside kill, and is reported as it is, like any
/// other error.
pub(super) fn interrupted_by(err: &Error, interrupt: &AtomicBool) -> bool {
    match err {
        Error::Interrupted => true,
        Error::Tool { code: None, .. } => interrupt.load(Ordering::SeqCst),
        _ => false,
    }
}

/// What a finished step did, when worth saying: how `join` joined, what `transcribe` found.
/// `Err` names the file that could not be read or parsed.
fn detail(step: StepName, ctx: &Ctx) -> std::result::Result<Option<String>, String> {
    match step {
        StepName::Join => {
            let probes: Vec<SourceProbe> = read_json(&layout::probe_file(&ctx.dir))?;
            let mode = match plan_join(&probes) {
                None => "single source",
                Some(Mode::StreamCopy) => "stream copy",
                Some(Mode::Reencode) => "re-encode",
            };
            Ok(Some(mode.to_string()))
        }
        StepName::Transcribe => {
            let transcript: Transcript = read_json(&layout::transcript_json(&ctx.dir))?;
            Ok(Some(format!(
                "language={}  {} segments",
                transcript.language,
                transcript.segments.len()
            )))
        }
        _ => Ok(None),
    }
}

/// Reads and parses the JSON file `file`; the error names it.
fn read_json<T: DeserializeOwned>(file: &Path) -> std::result::Result<T, String> {
    let text =
        fs::read_to_string(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("cannot parse {}: {e}", file.display()))
}

#[cfg(test)]
mod tests;
