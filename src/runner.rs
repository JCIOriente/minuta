//! The pipeline behind `run` and `transcribe`: template resolution, preflight, the job
//! folder, staleness, the five steps in order, then the templates in dependency order.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::chapters;
use crate::config::{Config, Setting, Source};
use crate::error::{Error, Missing, Result};
use crate::generate::{self, TemplateRun};
use crate::job::{
    self, plan_stale, step_fingerprint, template_reason, Ctx, Force, Job, Reason, Step, StepName,
};
use crate::lang;
use crate::llm::call::{CallCache, Policy};
use crate::llm::claude::{self, Claude};
use crate::llm::ollama::{self, Ollama};
use crate::llm::{Backend, BackendId};
use crate::models::{self, ModelKind};
use crate::progress::{template_detail, Progress};
use crate::steps::probe::read_probe;
use crate::steps::transcribe::TRANSCRIPT_FILE;
use crate::steps::{plan_join, Audio, Join, Mode, Outputs, Probe, Transcribe};
use crate::template::discover::{discover, Catalog, Dirs};
use crate::template::graph;
use crate::template::{Input, Output};
use crate::tools::{self, Report, RunOpts, ToolPaths, Toolchain};
use crate::transcript::Transcript;

/// What `run` and `transcribe` were asked to do, besides the settings in [`Config`]. The
/// `--llm` and `--output-lang` flags travel in [`Config`] as settings with [`Source::Flag`].
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Sources, in join order.
    pub files: Vec<PathBuf>,
    /// `--name`; the job name is derived from it, or else from the first source. It only
    /// picks the job: a job of that name made from other sources is never replaced.
    pub name: Option<String>,
    /// `--force`: what to rerun. It also lets a job made from other sources be replaced.
    pub force: Option<ForceArg>,
    /// `-t`: the requested templates, in flag order.
    pub templates: Vec<String>,
}

/// The value of `--force`, before it is resolved against the steps and the templates in the
/// run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForceArg {
    /// No value: every step and every template in the run.
    All,
    /// A step (it, every later step and every template in the run) or a template in the run
    /// (it and the templates that depend on it).
    Name(String),
}

/// Runs the pipeline, then the templates, and returns the job folder. Templates are resolved
/// first and preflight comes next, and the steps are set up before the job folder is opened,
/// so a bad template, a missing tool, model or backend, or a failing ffmpeg stops the run
/// before any file is touched. Steps and templates that are not stale are reported as cached
/// and not run. When `interrupt` is set the running tool or LLM call is stopped, temp files
/// are removed and the result is `Error::Interrupted`, with the unfinished step or template
/// left stale.
pub fn run(
    options: &Options,
    config: &Config,
    interrupt: &Arc<AtomicBool>,
    progress: &mut Progress,
) -> Result<PathBuf> {
    let started = Instant::now();
    let resolved = resolve(options, config)?;
    let ready = preflight(config, &resolved.backend_ids());
    if interrupt.load(Ordering::SeqCst) {
        return Err(Error::Interrupted);
    }
    let Ready {
        toolchain,
        vad_model,
        backends,
    } = ready?;
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
        vad_model.clone(),
    );
    let steps: [&dyn Runnable; 5] = [&probe, &join, &audio, &transcribe, &Outputs];

    for name in resolved.off_machine() {
        progress.notice(&format!("{name} sends its input to claude (Anthropic)"));
    }

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
    let plan = plan_stale(&job, &current, resolved.step_force);
    job.mark_stale(&plan)?;
    if vad_model.is_none() && plan.iter().any(|(step, _)| *step == StepName::Transcribe) {
        progress.note("VAD disabled (no VAD model configured)");
    }

    let total = steps.len() + resolved.order.len();
    progress.set_total(total);
    let pipeline = Pipeline {
        ctx: &ctx,
        plan: &plan,
        interrupt,
    };
    let templates = TemplatePhase {
        ctx: &ctx,
        job_name: &name,
        resolved: &resolved,
        backends: &backends,
        config,
        interrupt,
        first_index: steps.len() + 1,
    };
    let result = pipeline
        .run(&mut job, &steps, &current, progress)
        .and_then(|ran| Ok(ran + templates.run(&mut job, progress)?));
    match result {
        Ok(ran) => {
            progress.summary(ran, total - ran, started.elapsed().as_secs_f64());
            Ok(job.dir)
        }
        Err(Error::Interrupted) => {
            // Best effort: leftovers are also removed the next time the job is opened.
            let _ = job.clean_tmp();
            let _ = CallCache::new(&job.dir).clean_tmp();
            Err(Error::Interrupted)
        }
        Err(e) => Err(e),
    }
}

/// The templates of a run, resolved before preflight.
#[derive(Debug, Default)]
struct Resolved {
    catalog: Catalog,
    /// The templates in the run: the requested ones plus their inputs, dependencies first.
    order: Vec<String>,
    /// The backend of each template in `order`.
    backends: BTreeMap<String, BackendId>,
    /// The steps `--force` covers.
    step_force: Option<Force>,
    /// The templates `--force` covers.
    forced: Vec<String>,
    /// `--output-lang`.
    flag_lang: Option<String>,
    /// `llm.output_lang` from the environment or the config file.
    config_lang: Option<String>,
}

impl Resolved {
    /// Every distinct backend in the run, in the order first used.
    fn backend_ids(&self) -> Vec<BackendId> {
        let mut ids: Vec<BackendId> = Vec::new();
        for name in &self.order {
            let id = &self.backends[name];
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        ids
    }

    /// The templates whose backend sends their input off the machine, each once.
    fn off_machine(&self) -> Vec<&str> {
        self.order
            .iter()
            .filter(|name| self.backends[*name].sends_off_machine())
            .map(String::as_str)
            .collect()
    }
}

/// The setting's value when it came from a command-line flag.
fn from_flag(setting: &Setting) -> Option<&str> {
    (setting.source == Source::Flag).then_some(setting.value.as_str())
}

/// Loads the templates in the run, orders them, picks each one's backend, checks the
/// language codes and resolves `--force`. Every problem is an `Error::Usage`, except an I/O
/// error while reading a templates directory. Without `-t` no template is loaded.
fn resolve(options: &Options, config: &Config) -> Result<Resolved> {
    let llm_flag = from_flag(&config.llm_default)
        .map(str::parse::<BackendId>)
        .transpose()?;
    let flag_lang = config
        .llm_output_lang
        .as_ref()
        .and_then(from_flag)
        .map(String::from);
    if let Some(code) = &flag_lang {
        lang::validate(code)?;
    }
    let mut resolved = Resolved {
        flag_lang,
        ..Resolved::default()
    };
    if !options.templates.is_empty() {
        resolved.catalog = discover(&Dirs::standard()?)?;
        let catalog = &resolved.catalog;
        for name in &options.templates {
            catalog.get(name)?;
        }
        resolved.order = graph::order(&options.templates, &|name| deps_of(catalog, name))?;
        let default: BackendId = config.llm_default.value.parse()?;
        for name in &resolved.order {
            // Reports an invalid input template with its own error.
            let template = catalog.get(name)?;
            let id = generate::backend_id(llm_flag.as_ref(), template.llm.as_ref(), &default);
            resolved.backends.insert(name.clone(), id);
        }
        resolved.config_lang = match &config.llm_output_lang {
            Some(setting) if setting.source != Source::Flag => {
                lang::validate(&setting.value)?;
                Some(setting.value.clone())
            }
            _ => None,
        };
    }
    let (step_force, forced) = resolve_force(options.force.as_ref(), &resolved)?;
    resolved.step_force = step_force;
    resolved.forced = forced;
    Ok(resolved)
}

/// What template `name` reads, for [`graph`]: its `input` template, then `chapters` when its
/// fields declare it. A template that failed to load counts as reading the transcript, so it
/// lands in the order and is reported with its own error.
fn deps_of(catalog: &Catalog, name: &str) -> Option<Vec<String>> {
    match catalog.templates.get(name) {
        Some(template) => {
            let mut deps = Vec::new();
            if let Input::Template(dep) = &template.input {
                deps.push(dep.clone());
            }
            if let Output::Fields(spec) = &template.output {
                if spec.declares_chapters() && !deps.iter().any(|d| d == chapters::NAME) {
                    deps.push(chapters::NAME.to_string());
                }
            }
            Some(deps)
        }
        None => catalog
            .invalid
            .iter()
            .any(|i| i.path.file_stem().is_some_and(|stem| stem == name))
            .then(Vec::new),
    }
}

/// Splits `--force` into the steps and the templates it covers.
fn resolve_force(
    force: Option<&ForceArg>,
    resolved: &Resolved,
) -> Result<(Option<Force>, Vec<String>)> {
    let order = &resolved.order;
    match force {
        None => Ok((None, Vec::new())),
        Some(ForceArg::All) => Ok((Some(Force::All), order.clone())),
        Some(ForceArg::Name(name)) => match name.parse::<StepName>() {
            Ok(step) => Ok((Some(Force::From(step)), order.clone())),
            Err(e) if order.is_empty() => Err(e),
            Err(_) if order.contains(name) => {
                let deps_of = |n: &str| deps_of(&resolved.catalog, n);
                Ok((None, graph::dependents(order, name, &deps_of)))
            }
            Err(_) => Err(Error::Usage(format!(
                "unknown step `{name}`; expected one of probe, join, audio, transcribe, \
                 outputs, or a template in the run ({})",
                order.join(", ")
            ))),
        },
    }
}

/// What preflight found: the media tools, the VAD model and a ready backend per backend id
/// in the run.
pub(crate) struct Ready {
    toolchain: Toolchain,
    /// `None` when no VAD model is configured.
    vad_model: Option<PathBuf>,
    /// Keyed by backend id.
    backends: BTreeMap<String, Box<dyn Backend>>,
}

/// Resolves the toolchain for `config` and checks every backend in `backends`, listing every
/// missing tool, the model, a configured VAD model that is not a file and every unreachable
/// backend or missing LLM model at once.
pub(crate) fn preflight(config: &Config, backends: &[BackendId]) -> Result<Ready> {
    let mut missing = Vec::new();
    let toolchain = match tools::preflight(&tool_paths(config)?, true) {
        Ok(toolchain) => Some(toolchain),
        Err(Error::MissingTools(m)) => {
            missing.extend(m);
            None
        }
        Err(other) => return Err(other),
    };
    let vad_model = locate_vad_model(config, &mut missing)?;
    missing = with_pull_hint(missing, config);
    let mut ready: BTreeMap<String, Box<dyn Backend>> = BTreeMap::new();
    let mut claude_bin: Option<std::result::Result<PathBuf, Missing>> = None;
    for id in backends {
        let backend: std::result::Result<Box<dyn Backend>, Missing> = match id {
            BackendId::Ollama { model } => {
                let url = &config.ollama_url.value;
                ollama::check(url, model)
                    .map(|info| Box::new(Ollama::new(url, model, config.num_ctx(), &info)) as _)
            }
            BackendId::Claude { model } => {
                // One binary serves every claude model: look for it once.
                if claude_bin.is_none() {
                    claude_bin = Some(locate_claude(&config.claude_bin.value)?);
                }
                let bin = claude_bin.clone().expect("located above");
                bin.map(|bin| Box::new(Claude::new(bin, model.clone())) as _)
            }
        };
        match backend {
            Ok(backend) => {
                ready.insert(id.to_string(), backend);
            }
            Err(m) if missing.contains(&m) => {}
            Err(m) => missing.push(m),
        }
    }
    match toolchain {
        Some(toolchain) if missing.is_empty() => Ok(Ready {
            toolchain,
            vad_model,
            backends: ready,
        }),
        _ => Err(Error::MissingTools(missing)),
    }
}

/// The `claude` binary as an absolute path: the backend runs it from a temporary directory,
/// where a relative path would not resolve.
fn locate_claude(bin: &str) -> Result<std::result::Result<PathBuf, Missing>> {
    let info = match claude::check(bin) {
        Ok(info) => info,
        Err(m) => return Ok(Err(m)),
    };
    if info.path.is_absolute() {
        return Ok(Ok(info.path));
    }
    fs::canonicalize(&info.path).map(Ok).map_err(|e| {
        Error::Io(io::Error::new(
            e.kind(),
            format!("{}: {e}", info.path.display()),
        ))
    })
}

/// The status of the default backend for `doctor`: `ok`, `not reachable: …`,
/// `model missing: …` or `invalid: …`.
pub fn llm_status(config: &Config) -> String {
    let id = match config.llm_default.value.parse::<BackendId>() {
        Ok(id) => id,
        Err(Error::Usage(message)) => return format!("invalid: {message}"),
        Err(e) => return format!("invalid: {e}"),
    };
    match &id {
        BackendId::Ollama { model } => match ollama::check(&config.ollama_url.value, model) {
            Ok(_) => "ok".to_string(),
            // Only a model absent from /api/tags is missing; any other failure, such as
            // /api/show failing, means the model cannot be reached.
            Err(m) if m.hint == format!("ollama pull {model}") => {
                format!("model missing: {}", m.hint)
            }
            Err(m) => format!("not reachable: {}", m.hint),
        },
        BackendId::Claude { .. } => match claude::check(&config.claude_bin.value) {
            Ok(_) => "ok".to_string(),
            Err(m) => format!("not reachable: {}", m.hint),
        },
    }
}

/// What `doctor` shows for `config`.
pub fn inspect(config: &Config) -> Result<Report> {
    let mut report = tools::inspect(&tool_paths(config)?, true);
    locate_vad_model(config, &mut report.missing)?;
    report.missing = with_pull_hint(report.missing, config);
    Ok(report)
}

/// Maps the configured binaries and model onto [`ToolPaths`]. A model name from the catalog
/// resolves into the models dir; a value that is neither a file nor a catalog name is kept
/// as a path, so preflight reports it as missing.
pub fn tool_paths(config: &Config) -> Result<ToolPaths> {
    let model = config
        .whisper_model
        .as_ref()
        .map(|setting| model_path(&setting.value, ModelKind::Whisper))
        .transpose()?;
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

/// The config key that takes a model of `kind`.
fn model_key(kind: ModelKind) -> &'static str {
    match kind {
        ModelKind::Whisper => "whisper.model",
        ModelKind::Vad => "whisper.vad_model",
    }
}

/// A configured model of `kind`: a catalog name resolves into the models dir, anything else is
/// kept as a path. A catalog name of the other kind is `Error::Usage`.
fn model_path(value: &str, kind: ModelKind) -> Result<PathBuf> {
    let path =
        models::resolve(value, &models::default_dir()?).unwrap_or_else(|_| PathBuf::from(value));
    let from_catalog = path != Path::new(value);
    match models::CATALOG.iter().find(|m| m.name == value) {
        Some(spec) if from_catalog && spec.kind != kind => {
            let what = match spec.kind {
                ModelKind::Whisper => "a whisper model",
                ModelKind::Vad => "a VAD model",
            };
            Err(Error::Usage(format!(
                "`{value}` is {what}; set it as {}, not {}",
                model_key(spec.kind),
                model_key(kind)
            )))
        }
        _ => Ok(path),
    }
}

/// The configured `whisper.vad_model` as a path, resolved like `whisper.model`.
fn vad_model_path(config: &Config) -> Result<Option<PathBuf>> {
    config
        .whisper_vad_model
        .as_ref()
        .map(|setting| model_path(&setting.value, ModelKind::Vad))
        .transpose()
}

/// The name of a missing VAD model in [`Missing`].
const VAD_MODEL: &str = "vad model";

/// The configured VAD model, if it is a file. One that is configured but not a file is added
/// to `missing`; a metadata error other than not found is returned, naming the path.
fn locate_vad_model(config: &Config, missing: &mut Vec<Missing>) -> Result<Option<PathBuf>> {
    let Some(path) = vad_model_path(config)? else {
        return Ok(None);
    };
    match fs::metadata(&path) {
        Ok(meta) if meta.is_file() => return Ok(Some(path)),
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(Error::Io(io::Error::new(
                e.kind(),
                format!("{}: {e}", path.display()),
            )))
        }
    }
    missing.push(Missing {
        name: VAD_MODEL.to_string(),
        hint: format!("configured path {} is not a file", path.display()),
    });
    Ok(None)
}

/// A missing model or VAD model that is a catalog name only needs `minuta models pull <name>`.
fn with_pull_hint(mut missing: Vec<Missing>, config: &Config) -> Vec<Missing> {
    let settings = [
        ("model", &config.whisper_model),
        (VAD_MODEL, &config.whisper_vad_model),
    ];
    for (name, setting) in settings {
        let Some(setting) = setting else {
            continue;
        };
        if models::CATALOG.iter().any(|m| m.name == setting.value) {
            for item in missing.iter_mut().filter(|m| m.name == name) {
                item.hint = format!("run `minuta models pull {}`", setting.value);
            }
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
                progress.cached(index, name.as_str());
                continue;
            }
            if self.interrupt.load(Ordering::SeqCst) {
                return Err(Error::Interrupted);
            }
            progress.start(index, name.as_str());
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
            let detail = detail(*name, self.ctx);
            progress.done(index, name.as_str(), detail.as_deref(), seconds);
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

    fn interrupted_by(&self, err: &Error) -> bool {
        interrupted_by(err, self.interrupt)
    }
}

/// Whether `err` is the result of an interrupt: the tool was killed for it, or died from a
/// signal while the flag is set (the group kill raced the tool's own exit). Tools run in
/// their own process group, so the terminal's Ctrl-C never reaches them: a signal death
/// with the flag clear is a crash or an outside kill, and is reported as it is, like any
/// other error.
fn interrupted_by(err: &Error, interrupt: &AtomicBool) -> bool {
    match err {
        Error::Interrupted => true,
        Error::Tool { code: None, .. } => interrupt.load(Ordering::SeqCst),
        _ => false,
    }
}

/// The templates of a run, after the pipeline. Kept apart from [`Pipeline`]: templates are
/// recorded under `templates` in `job.json`, and their fingerprints can only be computed
/// once their input exists.
struct TemplatePhase<'a> {
    ctx: &'a Ctx,
    job_name: &'a str,
    resolved: &'a Resolved,
    backends: &'a BTreeMap<String, Box<dyn Backend>>,
    config: &'a Config,
    interrupt: &'a Arc<AtomicBool>,
    /// The progress index of the first template.
    first_index: usize,
}

impl TemplatePhase<'_> {
    /// Runs the stale templates in dependency order, marking each done as it finishes;
    /// returns how many ran. The first failure stops the run.
    fn run(&self, job: &mut Job, progress: &mut Progress) -> Result<usize> {
        if self.resolved.order.is_empty() {
            return Ok(0);
        }
        CallCache::new(&self.ctx.dir).clean_tmp()?;
        let detected = detected_language(&self.ctx.dir)?;
        let mut ran = 0;
        for (i, name) in self.resolved.order.iter().enumerate() {
            let index = self.first_index + i;
            if self.interrupt.load(Ordering::SeqCst) {
                return Err(Error::Interrupted);
            }
            match self.run_one(job, name, index, &detected, progress) {
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

    /// Runs template `name` when it is stale; returns whether it ran.
    fn run_one(
        &self,
        job: &mut Job,
        name: &str,
        index: usize,
        detected: &str,
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
        };
        let fingerprint = run.fingerprint(self.ctx, self.job_name)?;
        let Some(reason) = template_reason(job, name, &fingerprint, forced)? else {
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
        job.mark_template_done(name, &fingerprint, seconds, &run.artifacts())?;
        let detail = template_detail(&id, generated.calls);
        progress.done(index, name, Some(&detail), seconds);
        Ok(true)
    }
}

/// Parallel calls within a template: `llm.concurrency`, or 1 for Ollama (one local model
/// serves one request at a time) and 3 for Claude.
fn concurrency(config: &Config, id: &BackendId) -> usize {
    config.concurrency().unwrap_or(match id {
        BackendId::Ollama { .. } => 1,
        BackendId::Claude { .. } => 3,
    })
}

/// The speech language recorded in the job's transcript.
fn detected_language(dir: &Path) -> Result<String> {
    let path = dir.join(TRANSCRIPT_FILE);
    let json = fs::read_to_string(&path)
        .map_err(|e| Error::Io(io::Error::new(e.kind(), format!("{}: {e}", path.display()))))?;
    let transcript: Transcript = serde_json::from_str(&json)
        .map_err(|e| Error::Parse(format!("{}: {e}", path.display())))?;
    Ok(transcript.language)
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

    #[test]
    fn vad_model_unset_is_none() {
        assert_eq!(vad_model_path(&config(&[])).unwrap(), None);
    }

    #[test]
    fn catalog_vad_model_resolves_into_models_dir() {
        let path = vad_model_path(&config(&[("MINUTA_WHISPER_VAD_MODEL", "silero-v5.1.2")]));
        assert_eq!(
            path.unwrap(),
            Some(
                models::default_dir()
                    .unwrap()
                    .join("ggml-silero-v5.1.2.bin")
            )
        );
    }

    #[test]
    fn vad_model_path_is_kept() {
        let path = vad_model_path(&config(&[("MINUTA_WHISPER_VAD_MODEL", "/m/vad.bin")]));
        assert_eq!(path.unwrap(), Some(PathBuf::from("/m/vad.bin")));
    }

    #[test]
    fn pull_hint_for_each_model_uses_its_own_setting() {
        let missing = || {
            vec![
                Missing {
                    name: "model".into(),
                    hint: "configured path is not a file".into(),
                },
                Missing {
                    name: "vad model".into(),
                    hint: "configured path is not a file".into(),
                },
            ]
        };
        let both = config(&[
            ("MINUTA_WHISPER_MODEL", "small"),
            ("MINUTA_WHISPER_VAD_MODEL", "silero-v5.1.2"),
        ]);
        let hinted = with_pull_hint(missing(), &both);
        assert_eq!(hinted[0].hint, "run `minuta models pull small`");
        assert_eq!(hinted[1].hint, "run `minuta models pull silero-v5.1.2`");
        let vad_only = config(&[
            ("MINUTA_WHISPER_MODEL", "/m/x.bin"),
            ("MINUTA_WHISPER_VAD_MODEL", "silero-v5.1.2"),
        ]);
        let hinted = with_pull_hint(missing(), &vad_only);
        assert_eq!(hinted[0].hint, "configured path is not a file");
        assert_eq!(hinted[1].hint, "run `minuta models pull silero-v5.1.2`");
    }

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

    /// Templates `a`, `b` (input `a`) and `c` in the run; `c` uses claude.
    fn chain() -> Resolved {
        let mut catalog = Catalog::default();
        for (name, text) in [
            ("a", "A."),
            ("b", "---\ninput: a\n---\nB."),
            ("c", "---\nllm: claude\n---\nC."),
        ] {
            let template =
                crate::template::Template::parse(name, crate::template::Layer::Project, None, text)
                    .unwrap();
            catalog.templates.insert(name.to_string(), template);
        }
        let ollama = BackendId::Ollama { model: "m".into() };
        let backends = [
            ("a", ollama.clone()),
            ("b", ollama),
            ("c", BackendId::Claude { model: None }),
        ];
        Resolved {
            catalog,
            order: vec!["a".into(), "b".into(), "c".into()],
            backends: backends
                .into_iter()
                .map(|(n, id)| (n.to_string(), id))
                .collect(),
            ..Resolved::default()
        }
    }

    fn force(arg: Option<ForceArg>, resolved: &Resolved) -> (Option<Force>, Vec<String>) {
        resolve_force(arg.as_ref(), resolved).unwrap()
    }

    fn name(s: &str) -> Option<ForceArg> {
        Some(ForceArg::Name(s.to_string()))
    }

    #[test]
    fn force_resolution() {
        let r = chain();
        let all = vec!["a".to_string(), "b".into(), "c".into()];
        assert_eq!(force(None, &r), (None, vec![]));
        assert_eq!(
            force(Some(ForceArg::All), &r),
            (Some(Force::All), all.clone())
        );
        assert_eq!(
            force(name("audio"), &r),
            (Some(Force::From(StepName::Audio)), all)
        );
        assert_eq!(force(name("a"), &r), (None, vec!["a".into(), "b".into()]));
        assert_eq!(force(name("b"), &r), (None, vec!["b".into()]));
        assert_eq!(force(name("c"), &r), (None, vec!["c".into()]));
    }

    /// A catalog of `(name, template text)` pairs. The texts are parsed under a placeholder
    /// name, so `chapters` (reserved for the parser) can still be a catalog key.
    fn catalog_of(templates: &[(&str, &str)]) -> Catalog {
        let mut catalog = Catalog::default();
        for (name, text) in templates {
            let template = crate::template::Template::parse(
                "placeholder",
                crate::template::Layer::Project,
                None,
                text,
            )
            .unwrap();
            catalog.templates.insert(name.to_string(), template);
        }
        catalog
    }

    /// Points `name` at `dep` as its input, as `input: chapters` will once the parser allows it.
    fn read_input_from(catalog: &mut Catalog, name: &str, dep: &str) {
        catalog.templates.get_mut(name).unwrap().input = Input::Template(dep.to_string());
    }

    const YOUTUBE_LIKE: &str =
        "---\ninput: summary\noutput: fields\nfields: [body, chapters]\n---\nY.";

    #[test]
    fn deps_of_input_then_chapters() {
        let catalog = catalog_of(&[("summary", "S."), ("youtube", YOUTUBE_LIKE)]);
        assert_eq!(
            deps_of(&catalog, "youtube"),
            Some(vec!["summary".to_string(), "chapters".to_string()])
        );
    }

    #[test]
    fn deps_of_no_duplicate() {
        let mut catalog = catalog_of(&[("toc", YOUTUBE_LIKE)]);
        read_input_from(&mut catalog, "toc", "chapters");
        assert_eq!(deps_of(&catalog, "toc"), Some(vec!["chapters".to_string()]));
    }

    #[test]
    fn deps_of_without_the_field_is_the_input_only() {
        let catalog = catalog_of(&[
            ("summary", "S."),
            (
                "post",
                "---\ninput: summary\noutput: fields\nfields: [body, tags]\n---\nP.",
            ),
        ]);
        assert_eq!(deps_of(&catalog, "post"), Some(vec!["summary".to_string()]));
        assert_eq!(deps_of(&catalog, "summary"), Some(vec![]));
        assert_eq!(deps_of(&catalog, "zzz"), None);
    }

    #[test]
    fn force_chapters_covers_dependents() {
        let mut catalog = catalog_of(&[
            ("summary", "S."),
            ("chapters", "C."),
            ("youtube", YOUTUBE_LIKE),
            ("toc", "T."),
            ("linkedin", "---\ninput: summary\n---\nL."),
        ]);
        read_input_from(&mut catalog, "toc", "chapters");
        let resolved = Resolved {
            catalog,
            order: ["summary", "chapters", "youtube", "toc", "linkedin"]
                .map(String::from)
                .to_vec(),
            ..Resolved::default()
        };
        assert_eq!(
            force(name("chapters"), &resolved),
            (
                None,
                vec!["chapters".to_string(), "youtube".into(), "toc".into()]
            )
        );
    }

    #[test]
    fn force_unknown_name_is_usage() {
        let err = resolve_force(name("zzz").as_ref(), &chain()).unwrap_err();
        assert!(
            matches!(&err, Error::Usage(m) if m.contains("`zzz`") && m.contains("(a, b, c)")),
            "{err}"
        );
        let err = resolve_force(name("zzz").as_ref(), &Resolved::default()).unwrap_err();
        assert!(
            matches!(&err, Error::Usage(m) if m.starts_with("unknown step `zzz`")),
            "{err}"
        );
    }

    #[test]
    fn distinct_backends_and_off_machine_templates() {
        let r = chain();
        assert_eq!(
            r.backend_ids(),
            [
                BackendId::Ollama { model: "m".into() },
                BackendId::Claude { model: None }
            ]
        );
        assert_eq!(r.off_machine(), ["c"]);
    }

    #[test]
    fn preflight_without_templates_checks_no_backend() {
        // Missing media tools, but no backend: only the tools are reported.
        let cfg = config(&[
            ("MINUTA_FFMPEG_BIN", "/nonexistent/ffmpeg"),
            ("MINUTA_OLLAMA_URL", "http://127.0.0.1:9"),
        ]);
        let Err(Error::MissingTools(missing)) = preflight(&cfg, &[]) else {
            panic!("expected missing tools");
        };
        assert!(missing.iter().all(|m| !m.name.starts_with("ollama")));
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

    #[test]
    fn signal_death_without_flag_is_reported_as_is() {
        let interrupt = Arc::new(AtomicBool::new(false));
        let killed = Error::Tool {
            tool: "ffmpeg".into(),
            code: None,
            stderr: String::new(),
        };
        assert!(!interrupted_by(&killed, &interrupt));
    }
}
