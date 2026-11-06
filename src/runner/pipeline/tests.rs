use super::*;
use crate::job::plan_stale;
use crate::progress::Mode as ProgressMode;
use crate::runner::test_support::probed_job;
use crate::tools;
use std::cell::Cell;
use std::io;
use std::path::Path;

#[test]
fn probed_seconds_sums_the_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = probed_job(tmp.path(), &[30.5, 29.25]);
    assert_eq!(probed_seconds(&ctx).unwrap(), Some(59.75));
}

#[test]
fn probed_seconds_of_zero_length_is_unknown() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = probed_job(tmp.path(), &[0.0]);
    assert_eq!(probed_seconds(&ctx).unwrap(), None);
}

#[test]
fn probed_seconds_without_probe_file_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = Ctx {
        dir: tmp.path().to_path_buf(),
        sources: vec![],
        source_digest: "d".into(),
    };
    match probed_seconds(&ctx).unwrap_err() {
        Error::Io(e) => assert_eq!(e.kind(), io::ErrorKind::NotFound),
        other => panic!("expected Io error, got {other:?}"),
    }
}

#[test]
fn join_and_audio_report_probed_seconds() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = probed_job(tmp.path(), &[30.0, 30.0]);
    let ffmpeg = tools::ToolInfo {
        path: PathBuf::from("/no/ffmpeg"),
        version: "ffmpeg 0".into(),
    };
    let join = Join::new(ffmpeg.clone(), crate::steps::Encoder::X264);
    assert_eq!(Runnable::media_seconds(&join, &ctx).unwrap(), Some(60.0));
    let audio = Audio::new(ffmpeg);
    assert_eq!(Runnable::media_seconds(&audio, &ctx).unwrap(), Some(60.0));
}

#[test]
fn transcribe_reports_no_media_seconds() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = probed_job(tmp.path(), &[30.0]);
    let whisper = tools::ToolInfo {
        path: PathBuf::from("/no/whisper-cli"),
        version: "whisper 0".into(),
    };
    let step = Transcribe::new(whisper, PathBuf::from("/no/model.bin"), "en", None);
    assert_eq!(Runnable::media_seconds(&step, &ctx).unwrap(), None);
}

/// A step that records whether it ran, can set the interrupt flag while running, sends
/// `lines` to `on_line` and reports `media` as its media seconds (`Err` fails with it).
struct FakeStep {
    ran: Cell<bool>,
    sets: Option<Arc<AtomicBool>>,
    lines: &'static [&'static str],
    media: std::result::Result<Option<f64>, &'static str>,
}

impl Default for FakeStep {
    fn default() -> FakeStep {
        FakeStep {
            ran: Cell::new(false),
            sets: None,
            lines: &[],
            media: Ok(None),
        }
    }
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
    fn execute(&self, ctx: &Ctx, _: RunOpts, on_line: &mut dyn FnMut(&str)) -> Result<Vec<String>> {
        for line in self.lines {
            on_line(line);
        }
        self.run(ctx).map(|()| Vec::new())
    }

    fn media_seconds(&self, _: &Ctx) -> Result<Option<f64>> {
        self.media.map_err(|e| Error::Io(io::Error::other(e)))
    }
}

/// Runs `step` as the only stale step of a fresh job with `interrupt`.
fn run_one(step: &FakeStep, interrupt: &Arc<AtomicBool>) -> (Result<usize>, Job) {
    let (result, job, _) = run_one_shown(step, interrupt, ProgressMode::Quiet);
    (result, job)
}

/// [`run_one`] with progress in `mode`, also returning the progress to see what it showed.
fn run_one_shown(
    step: &FakeStep,
    interrupt: &Arc<AtomicBool>,
    mode: ProgressMode,
) -> (Result<usize>, Job, Progress) {
    run_as(StepName::Probe, step, interrupt, mode, |_| {})
}

/// [`run_one_shown`] with the step run as `name`, after `prepare` got the job folder.
fn run_as(
    name: StepName,
    step: &FakeStep,
    interrupt: &Arc<AtomicBool>,
    mode: ProgressMode,
    prepare: impl FnOnce(&Path),
) -> (Result<usize>, Job, Progress) {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("a.mp4");
    fs::write(&source, "media").unwrap();
    let mut job = Job::open(tmp.path(), "a", &[source], false).unwrap();
    let ctx = job.ctx();
    prepare(&ctx.dir);
    let current = vec![(name, step.fingerprint(&ctx, None))];
    let plan = plan_stale(&job, &current, None);
    let pipeline = Pipeline {
        ctx: &ctx,
        plan: &plan,
        interrupt,
    };
    let mut progress = Progress::new(mode);
    let result = pipeline.run(&mut job, &[step], &current, &mut progress);
    (result, job, progress)
}

#[test]
fn spinner_shows_percent_of_the_step_media_seconds() {
    let step = FakeStep {
        lines: &["frame=  662 fps=0.0 q=28.0 size=  256KiB time=00:00:27.40 speed=33.7x"],
        media: Ok(Some(60.0)),
        ..FakeStep::default()
    };
    let interrupt = Arc::new(AtomicBool::new(false));
    let (result, _, progress) = run_one_shown(&step, &interrupt, ProgressMode::Live);
    assert_eq!(result.unwrap(), 1);
    // The spinner message, then the step's done line (its time varies).
    assert_eq!(progress.shown.len(), 2, "{:?}", progress.shown);
    assert_eq!(progress.shown[0], "45%  00:27/01:00");
}

#[test]
fn media_seconds_error_runs_the_step_with_raw_progress() {
    let step = FakeStep {
        lines: &["frame=  662 fps=0.0 q=28.0 size=  256KiB time=00:00:27.40 speed=33.7x"],
        media: Err("probe.json unreadable"),
        ..FakeStep::default()
    };
    let interrupt = Arc::new(AtomicBool::new(false));
    let (result, job, progress) = run_one_shown(&step, &interrupt, ProgressMode::Live);
    assert_eq!(result.unwrap(), 1);
    assert!(step.ran.get());
    assert!(job.state.steps.contains_key(&StepName::Probe));
    // No percent without a total: the spinner shows ffmpeg's own line, then the done line.
    assert_eq!(progress.shown.len(), 2, "{:?}", progress.shown);
    assert_eq!(
        progress.shown[0],
        "frame=  662 fps=0.0 q=28.0 size=  256KiB time=00:00:27.40 speed=33.7x"
    );
}

#[test]
fn preset_interrupt_runs_no_step() {
    let step = FakeStep::default();
    let (result, job) = run_one(&step, &Arc::new(AtomicBool::new(true)));
    assert!(matches!(result, Err(Error::Interrupted)), "{result:?}");
    assert!(!step.ran.get());
    assert!(!job.state.steps.contains_key(&StepName::Probe));
}

#[test]
fn interrupt_during_the_last_step_is_reported() {
    let interrupt = Arc::new(AtomicBool::new(false));
    let step = FakeStep {
        sets: Some(Arc::clone(&interrupt)),
        ..FakeStep::default()
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

/// Runs a fake step as `name` in plain mode after `prepare`; returns the job folder, the
/// done line without its time, and the lines shown after it. The run must succeed and mark
/// the step done.
fn run_detail(name: StepName, prepare: impl FnOnce(&Path)) -> (PathBuf, String, Vec<String>) {
    let interrupt = Arc::new(AtomicBool::new(false));
    let step = FakeStep::default();
    let (result, job, progress) = run_as(name, &step, &interrupt, ProgressMode::Plain, prepare);
    assert_eq!(result.unwrap(), 1);
    assert!(job.state.steps.contains_key(&name));
    let (done, rest) = progress.shown.split_first().expect("a done line");
    let (line, time) = done.rsplit_once(' ').expect("a time");
    assert!(time.ends_with('s'), "done line was {done:?}");
    (job.dir, line.trim_end().to_string(), rest.to_vec())
}

#[test]
fn transcribe_detail_shows_language_and_segments() {
    let (_, line, rest) = run_detail(StepName::Transcribe, |dir| {
        let transcript = r#"{"language":"en","segments":[
            {"start":0.0,"end":1.0,"text":"a"},{"start":1.0,"end":2.0,"text":"b"}]}"#;
        fs::create_dir_all(layout::transcript_json(dir).parent().unwrap()).unwrap();
        fs::write(layout::transcript_json(dir), transcript).unwrap();
    });
    assert_eq!(line, "[1/5] transcribe  language=en  2 segments");
    assert!(rest.is_empty(), "{rest:?}");
}

#[test]
fn transcribe_detail_with_unreadable_transcript_warns() {
    let (dir, line, rest) = run_detail(StepName::Transcribe, |_| {});
    assert_eq!(line, "[1/5] transcribe");
    let file = layout::transcript_json(&dir);
    assert_eq!(
        rest,
        [format!(
            "warning: transcribe: cannot read {}: No such file or directory (os error 2)",
            file.display()
        )]
    );
}

#[test]
fn transcribe_detail_with_malformed_transcript_warns() {
    let (dir, line, rest) = run_detail(StepName::Transcribe, |dir| {
        fs::create_dir_all(layout::transcript_json(dir).parent().unwrap()).unwrap();
        fs::write(layout::transcript_json(dir), "{").unwrap();
    });
    assert_eq!(line, "[1/5] transcribe");
    let file = layout::transcript_json(&dir);
    assert_eq!(
        rest,
        [format!(
            "warning: transcribe: cannot parse {}: EOF while parsing an object at line 1 column 1",
            file.display()
        )]
    );
}

#[test]
fn join_detail_shows_single_source() {
    let (_, line, rest) = run_detail(StepName::Join, |dir| {
        probed_job(dir, &[30.0]);
    });
    assert_eq!(line, "[1/5] join        single source");
    assert!(rest.is_empty(), "{rest:?}");
}

#[test]
fn join_detail_with_unreadable_probe_warns() {
    let (dir, line, rest) = run_detail(StepName::Join, |_| {});
    assert_eq!(line, "[1/5] join");
    let file = layout::probe_file(&dir);
    assert_eq!(
        rest,
        [format!(
            "warning: join: cannot read {}: No such file or directory (os error 2)",
            file.display()
        )]
    );
}
