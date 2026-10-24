//! The pipeline behind `run` and `transcribe`: preflight, the job folder, staleness and the
//! five steps in order.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::error::{Error, Missing, Result};
use crate::job::{self, plan_stale, step_fingerprint, Ctx, Force, Job, Reason, Step, StepName};
use crate::models;
use crate::progress::Progress;
use crate::steps::probe::read_probe;
use crate::steps::transcribe::TRANSCRIPT_FILE;
use crate::steps::{plan_join, Audio, Join, Mode, Outputs, Probe, Transcribe};
use crate::tools::{self, Report, RunOpts, ToolPaths, Toolchain};
use crate::transcript::Transcript;

/// How long to wait for the Ctrl-C handler after a tool died from a signal.
const SIGNAL_GRACE: Duration = Duration::from_millis(100);

/// What `run` and `transcribe` were asked to do, besides the settings in [`Config`].
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Sources, in join order.
    pub files: Vec<PathBuf>,
    /// `--name`; the job name is derived from it, or else from the first source. It only
    /// picks the job: a job of that name made from other sources is never replaced.
    pub name: Option<String>,
    /// `--force`: the steps to rerun. It also lets a job made from other sources be replaced.
    pub force: Option<Force>,
}

/// Runs the pipeline and returns the job folder. Preflight comes first, and the steps are set
/// up before the job folder is opened, so a missing tool or model or a failing ffmpeg stops
/// the run before any file is touched. Steps that are not stale are reported as cached and
/// not run. When `interrupt` is set the running tool is killed, temp files are removed and
/// the result is `Error::Interrupted`, with the unfinished step left stale.
pub fn run(
    options: &Options,
    config: &Config,
    interrupt: &Arc<AtomicBool>,
    progress: &mut Progress,
) -> Result<PathBuf> {
    let started = Instant::now();
    let toolchain = preflight(config);
    if interrupt.load(Ordering::SeqCst) {
        return Err(Error::Interrupted);
    }
    let toolchain = toolchain?;
    let first = options
        .files
        .first()
        .ok_or_else(|| Error::Usage("no source files given".into()))?;
    if let Some(missing) = options.files.iter().find(|f| !f.is_file()) {
        return Err(Error::Usage(format!(
            "source file not found: {}",
            missing.display()
        )));
    }
    let name = job::derive_name(options.name.as_deref().map_or(first.as_path(), Path::new))?;

    let model = toolchain
        .model
        .clone()
        .expect("preflight requires the model");
    let probe = Probe::new(toolchain.ffprobe.clone());
    // Runs `ffmpeg -encoders` on every run, cached or not: the encoder is part of the join
    // fingerprint, so it is needed to decide whether join is stale.
    let join = Join::detect(toolchain.ffmpeg.clone())?;
    let audio = Audio::new(toolchain.ffmpeg.clone());
    let transcribe = Transcribe::new(
        toolchain.whisper.clone(),
        model,
        config.whisper_lang.value.clone(),
        None,
    );
    let steps: [&dyn Runnable; 5] = [&probe, &join, &audio, &transcribe, &Outputs];

    let mut job = Job::open(
        Path::new(&config.out_dir.value),
        &name,
        &options.files,
        options.force.is_some(),
    )?;
    let ctx = job.ctx();

    let mut current: Vec<(StepName, String)> = Vec::new();
    for step in steps {
        let previous = current.last().map(|(_, fingerprint)| fingerprint.as_str());
        let fingerprint = step.fingerprint(&ctx, previous);
        current.push((step.name(), fingerprint));
    }
    let plan = plan_stale(&job, &current, options.force);
    job.mark_stale(&plan)?;
    if plan.iter().any(|(step, _)| *step == StepName::Transcribe) {
        progress.note("VAD disabled (no VAD model configured)");
    }

    let pipeline = Pipeline {
        ctx: &ctx,
        plan: &plan,
        interrupt,
    };
    match pipeline.run(&mut job, &steps, &current, progress) {
        Ok(ran) => {
            progress.summary(ran, steps.len() - ran, started.elapsed().as_secs_f64());
            Ok(job.dir)
        }
        Err(Error::Interrupted) => {
            // Best effort: leftovers are also removed the next time the job is opened.
            let _ = job.clean_tmp();
            Err(Error::Interrupted)
        }
        Err(e) => Err(e),
    }
}

/// Resolves the toolchain for `config`, listing every missing tool and the model at once.
pub fn preflight(config: &Config) -> Result<Toolchain> {
    tools::preflight(&tool_paths(config)?, true).map_err(|e| match e {
        Error::MissingTools(missing) => Error::MissingTools(with_pull_hint(missing, config)),
        other => other,
    })
}

/// What `doctor` shows for `config`.
pub fn inspect(config: &Config) -> Result<Report> {
    let mut report = tools::inspect(&tool_paths(config)?, true);
    report.missing = with_pull_hint(report.missing, config);
    Ok(report)
}

/// Maps the configured binaries and model onto [`ToolPaths`]. A model name from the catalog
/// resolves into the models dir; a value that is neither a file nor a catalog name is kept
/// as a path, so preflight reports it as missing.
pub fn tool_paths(config: &Config) -> Result<ToolPaths> {
    let model = match &config.whisper_model {
        Some(setting) => Some(
            models::resolve(&setting.value, &models::default_dir()?)
                .unwrap_or_else(|_| PathBuf::from(&setting.value)),
        ),
        None => None,
    };
    Ok(ToolPaths {
        ffmpeg: bin_path(&config.ffmpeg_bin.value, "ffmpeg"),
        ffprobe: bin_path(&config.ffprobe_bin.value, "ffprobe"),
        whisper: bin_path(&config.whisper_bin.value, "whisper-cli"),
        model,
        path: None,
    })
}

/// A configured binary. The tool's own name is left to the `PATH` lookup in `tools`, another
/// bare name is looked up on `PATH` here, and a value with a directory part is used as is.
fn bin_path(value: &str, tool: &str) -> Option<PathBuf> {
    if value == tool {
        return None;
    }
    let path = Path::new(value);
    if path.components().count() > 1 {
        return Some(path.to_path_buf());
    }
    let found = std::env::var_os("PATH").and_then(|search| {
        std::env::split_paths(&search)
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join(value))
            .find(|candidate| candidate.is_file())
    });
    Some(found.unwrap_or_else(|| path.to_path_buf()))
}

/// A missing model that is a catalog name only needs `minuta models pull <name>`.
fn with_pull_hint(mut missing: Vec<Missing>, config: &Config) -> Vec<Missing> {
    let Some(setting) = &config.whisper_model else {
        return missing;
    };
    if models::CATALOG.iter().any(|m| m.name == setting.value) {
        for item in missing.iter_mut().filter(|m| m.name == "model") {
            item.hint = format!("run `minuta models pull {}`", setting.value);
        }
    }
    missing
}

/// The progress-aware entry point of a step; returns warnings to show.
trait Execute {
    fn execute(
        &self,
        ctx: &Ctx,
        opts: RunOpts,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<Vec<String>>;
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
trait Runnable {
    fn name(&self) -> StepName;
    fn fingerprint(&self, ctx: &Ctx, previous: Option<&str>) -> String;
    fn artifacts(&self, ctx: &Ctx) -> Vec<PathBuf>;
    fn execute(
        &self,
        ctx: &Ctx,
        opts: RunOpts,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<Vec<String>>;
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
}

struct Pipeline<'a> {
    ctx: &'a Ctx,
    plan: &'a [(StepName, Reason)],
    interrupt: &'a Arc<AtomicBool>,
}

impl Pipeline<'_> {
    /// Runs the stale steps in order, marking each done as it finishes; returns how many ran.
    fn run(
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
                progress.cached(index, *name);
                continue;
            }
            if self.interrupt.load(Ordering::SeqCst) {
                return Err(Error::Interrupted);
            }
            progress.start(index, *name);
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
                    progress.failed(index, *name, matches!(e, Error::Interrupted));
                    return Err(e);
                }
            };
            let seconds = started.elapsed().as_secs_f64();
            job.mark_done(*name, fingerprint, seconds, &step.artifacts(self.ctx))?;
            progress.done(index, *name, detail(*name, self.ctx).as_deref(), seconds);
            for warning in &warnings {
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

    /// Whether `err` is the result of Ctrl-C: the tool was killed for the interrupt, or died
    /// from a signal while the flag is set. The terminal signals the whole process group, so
    /// the tool can die before the handler has set the flag; give it a moment then. Any other
    /// error is reported as it is.
    fn interrupted_by(&self, err: &Error) -> bool {
        match err {
            Error::Interrupted => true,
            Error::Tool { code: None, .. } => {
                if !self.interrupt.load(Ordering::SeqCst) {
                    std::thread::sleep(SIGNAL_GRACE);
                }
                self.interrupt.load(Ordering::SeqCst)
            }
            _ => false,
        }
    }
}

/// What a finished step did, when worth saying: how `join` joined, what `transcribe` found.
fn detail(step: StepName, ctx: &Ctx) -> Option<String> {
    match step {
        StepName::Join => {
            let mode = match plan_join(&read_probe(&ctx.dir).ok()?) {
                Mode::Link => "single source",
                Mode::StreamCopy => "stream copy",
                Mode::Reencode => "re-encode",
            };
            Some(mode.to_string())
        }
        StepName::Transcribe => {
            let json = fs::read_to_string(ctx.dir.join(TRANSCRIPT_FILE)).ok()?;
            let transcript: Transcript = serde_json::from_str(&json).ok()?;
            Some(format!(
                "language={}  {} segments",
                transcript.language,
                transcript.segments.len()
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Overrides;
    use crate::progress::Mode as ProgressMode;
    use std::cell::Cell;

    fn config(env: &[(&str, &str)]) -> Config {
        let lookup = |key: &str| {
            env.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        };
        Config::resolve(Overrides::default(), &lookup, None).unwrap()
    }

    #[test]
    fn default_binaries_are_looked_up_by_tools() {
        let paths = tool_paths(&config(&[])).unwrap();
        assert_eq!(paths.ffmpeg, None);
        assert_eq!(paths.ffprobe, None);
        assert_eq!(paths.whisper, None);
        assert_eq!(paths.model, None);
        assert_eq!(paths.path, None);
    }

    #[test]
    fn configured_paths_are_used_as_is() {
        let paths = tool_paths(&config(&[
            ("MINUTA_FFMPEG_BIN", "/opt/ff/ffmpeg"),
            ("MINUTA_WHISPER_BIN", "./bin/whisper-cli"),
            ("MINUTA_FFPROBE_BIN", "no-such-tool-anywhere"),
        ]))
        .unwrap();
        assert_eq!(paths.ffmpeg, Some(PathBuf::from("/opt/ff/ffmpeg")));
        assert_eq!(paths.whisper, Some(PathBuf::from("./bin/whisper-cli")));
        // Not on PATH: kept, so preflight names it as the configured path.
        assert_eq!(paths.ffprobe, Some(PathBuf::from("no-such-tool-anywhere")));
    }

    #[test]
    fn catalog_model_resolves_into_models_dir() {
        let paths = tool_paths(&config(&[("MINUTA_WHISPER_MODEL", "base")])).unwrap();
        assert_eq!(
            paths.model,
            Some(models::default_dir().unwrap().join("ggml-base.bin"))
        );
        let paths = tool_paths(&config(&[("MINUTA_WHISPER_MODEL", "/m/x.bin")])).unwrap();
        assert_eq!(paths.model, Some(PathBuf::from("/m/x.bin")));
    }

    #[test]
    fn pull_hint_only_for_catalog_names() {
        let missing = || {
            vec![Missing {
                name: "model".into(),
                hint: "configured path is not a file".into(),
            }]
        };
        let hinted = with_pull_hint(missing(), &config(&[("MINUTA_WHISPER_MODEL", "small")]));
        assert_eq!(hinted[0].hint, "run `minuta models pull small`");
        let kept = with_pull_hint(missing(), &config(&[("MINUTA_WHISPER_MODEL", "/m/x.bin")]));
        assert_eq!(kept, missing());
    }

    /// A step that records whether it ran and can set the interrupt flag while running.
    struct FakeStep {
        ran: Cell<bool>,
        sets: Option<Arc<AtomicBool>>,
    }

    impl Step for FakeStep {
        const NAME: StepName = StepName::Probe;
        fn params(&self, _: &Ctx) -> Vec<u8> {
            Vec::new()
        }
        fn outputs(&self, _: &Ctx) -> Vec<PathBuf> {
            Vec::new()
        }
        fn run(&self, _: &Ctx) -> Result<()> {
            self.ran.set(true);
            if let Some(flag) = &self.sets {
                flag.store(true, Ordering::SeqCst);
            }
            Ok(())
        }
    }

    impl Execute for FakeStep {
        fn execute(&self, ctx: &Ctx, _: RunOpts, _: &mut dyn FnMut(&str)) -> Result<Vec<String>> {
            self.run(ctx).map(|()| Vec::new())
        }
    }

    /// Runs `step` as the only stale step of a fresh job with `interrupt`.
    fn run_one(step: &FakeStep, interrupt: &Arc<AtomicBool>) -> (Result<usize>, Job) {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("a.mp4");
        fs::write(&source, "media").unwrap();
        let mut job = Job::open(tmp.path(), "a", &[source], false).unwrap();
        let ctx = job.ctx();
        let current = vec![(StepName::Probe, step.fingerprint(&ctx, None))];
        let plan = plan_stale(&job, &current, None);
        let pipeline = Pipeline {
            ctx: &ctx,
            plan: &plan,
            interrupt,
        };
        let mut progress = Progress::new(ProgressMode::Quiet);
        let result = pipeline.run(&mut job, &[step], &current, &mut progress);
        (result, job)
    }

    #[test]
    fn preset_interrupt_runs_no_step() {
        let step = FakeStep {
            ran: Cell::new(false),
            sets: None,
        };
        let (result, job) = run_one(&step, &Arc::new(AtomicBool::new(true)));
        assert!(matches!(result, Err(Error::Interrupted)), "{result:?}");
        assert!(!step.ran.get());
        assert!(!job.state.steps.contains_key(&StepName::Probe));
    }

    #[test]
    fn interrupt_during_the_last_step_is_reported() {
        let interrupt = Arc::new(AtomicBool::new(false));
        let step = FakeStep {
            ran: Cell::new(false),
            sets: Some(Arc::clone(&interrupt)),
        };
        let (result, _) = run_one(&step, &interrupt);
        assert!(step.ran.get());
        assert!(matches!(result, Err(Error::Interrupted)), "{result:?}");
    }

    #[test]
    fn only_signal_deaths_count_as_interrupts() {
        let interrupt = Arc::new(AtomicBool::new(true));
        let (ctx, plan) = (
            Ctx {
                dir: PathBuf::new(),
                sources: Vec::new(),
                source_digest: String::new(),
            },
            Vec::new(),
        );
        let pipeline = Pipeline {
            ctx: &ctx,
            plan: &plan,
            interrupt: &interrupt,
        };
        let killed = Error::Tool {
            tool: "ffmpeg".into(),
            code: None,
            stderr: String::new(),
        };
        assert!(pipeline.interrupted_by(&killed));
        assert!(pipeline.interrupted_by(&Error::Interrupted));
        assert!(!pipeline.interrupted_by(&Error::Io(std::io::Error::other("disk full"))));
        let failed = Error::Tool {
            tool: "ffmpeg".into(),
            code: Some(1),
            stderr: String::new(),
        };
        assert!(!pipeline.interrupted_by(&failed));
    }
}
