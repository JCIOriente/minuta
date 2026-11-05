//! The pipeline behind `run` and `transcribe`: template resolution, preflight, the job
//! folder, staleness, the five steps in order, then the templates in dependency order.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::chapters;
use crate::config::{Config, Setting, Source};
use crate::error::{Error, Result};
use crate::generate;
use crate::job::{self, plan_stale, Force, Job, StepName};
use crate::lang;
use crate::llm::call::CallCache;
use crate::llm::BackendId;
use crate::progress::Progress;
use crate::steps::{Audio, Join, Outputs, Probe, Transcribe};
use crate::template::discover::{discover, Catalog, Dirs};
use crate::template::graph;
use crate::template::{Input, Output};

mod pipeline;
mod preflight;
mod templates;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;

use pipeline::{Pipeline, Runnable};
pub use preflight::{inspect, llm_status, tool_paths};
pub(crate) use preflight::{preflight, Ready};
use templates::TemplatePhase;

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

    for (name, target) in resolved.off_machine(config) {
        progress.notice(&format!("{name} sends its input to {target}"));
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
    /// The templates named with `-t`. `--force` does not request one. The other
    /// templates in `order` are implicit: they run only because a requested one needs them.
    requested: Vec<String>,
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

    /// The templates whose backend sends their input off the machine, each once, with
    /// where it goes, such as `claude (Anthropic)` or `openrouter (openrouter.ai)`.
    fn off_machine(&self, config: &Config) -> Vec<(&str, String)> {
        self.order
            .iter()
            .filter_map(|name| {
                let id = &self.backends[name];
                if !id.sends_off_machine() {
                    return None;
                }
                if let BackendId::Endpoint { name: endpoint, .. } = id {
                    let found = config.endpoints.get(endpoint)?;
                    return (!found.is_local())
                        .then(|| (name.as_str(), format!("{endpoint} ({})", found.host())));
                }
                let provider = id.provider()?;
                let tool = id.to_string();
                let tool = tool.split(':').next().unwrap_or_default();
                Some((name.as_str(), format!("{tool} ({provider})")))
            })
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
    // Without a template the flag reaches no backend, so preflight would not see a typo.
    if let Some(BackendId::Endpoint { name, .. }) = &llm_flag {
        config.endpoint(name)?;
    }
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
    resolved.requested = options.templates.clone();
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
