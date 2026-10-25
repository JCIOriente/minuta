use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use minuta::error::Error;
use minuta::job::{
    derive_name, fingerprint, plan_stale, source_record, step_fingerprint, template_reason,
    write_atomic, Ctx, Force, Job, Reason, Source, Status, Step, StepName,
};
use tempfile::TempDir;

/// A temp dir holding one source file `clase-4.mp4` and an empty output dir.
fn setup() -> (TempDir, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("clase-4.mp4");
    fs::write(&source, b"source bytes").unwrap();
    let out = root.path().join("minuta-out");
    (root, out, source)
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// Fingerprints `fp-<step>` for every step, in pipeline order.
fn current() -> Vec<(StepName, String)> {
    StepName::ALL
        .iter()
        .map(|&step| (step, format!("fp-{step}")))
        .collect()
}

/// A job where every step is done with fingerprint `fp-<step>` and artifact `<step>.out`.
fn done_job() -> (TempDir, Job) {
    let (root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    for (step, fp) in current() {
        let artifact = job.dir.join(format!("{step}.out"));
        fs::write(&artifact, step.as_str()).unwrap();
        job.mark_done(step, &fp, 0.1, &[artifact]).unwrap();
    }
    assert!(plan_stale(&job, &current(), None).is_empty());
    (root, job)
}

/// `step` stale for `reason`, and every later step stale because of it.
fn stale_from(step: StepName, reason: Reason) -> Vec<(StepName, Reason)> {
    StepName::ALL
        .into_iter()
        .filter(|&s| s >= step)
        .map(|s| {
            (
                s,
                if s == step {
                    reason.clone()
                } else {
                    Reason::Upstream(step)
                },
            )
        })
        .collect()
}

#[test]
fn derive_name_strips_extension() {
    assert_eq!(derive_name(Path::new("clase-4.mp4")).unwrap(), "clase-4");
    assert_eq!(
        derive_name(Path::new("/videos/clase-4.mp4")).unwrap(),
        "clase-4"
    );
    assert_eq!(derive_name(Path::new("clase-4")).unwrap(), "clase-4");
    assert_eq!(derive_name(Path::new("a.b.mkv")).unwrap(), "a.b");
    assert_eq!(derive_name(Path::new("x.mp4.wav")).unwrap(), "x");
    for ext in [
        "mp4", "mkv", "mov", "m4v", "webm", "mp3", "wav", "m4a", "flac",
    ] {
        assert_eq!(
            derive_name(Path::new(&format!("talk.{ext}"))).unwrap(),
            "talk"
        );
    }
    assert_eq!(derive_name(Path::new("notes.txt")).unwrap(), "notes.txt");
}

#[test]
fn derive_name_case_insensitive() {
    assert_eq!(derive_name(Path::new("CLASE-4.MP4")).unwrap(), "CLASE-4");
    assert_eq!(derive_name(Path::new("talk.Mov")).unwrap(), "talk");
    assert_eq!(derive_name(Path::new("talk.FlAc")).unwrap(), "talk");
}

#[test]
fn derive_name_without_file_name_is_usage_error() {
    for path in ["", "..", "/", "videos/.."] {
        assert!(
            matches!(derive_name(Path::new(path)), Err(Error::Usage(_))),
            "{path:?}"
        );
    }
}

#[test]
fn invalid_job_names_rejected() {
    let (_root, out, source) = setup();
    for name in ["", ".", "..", "a/b"] {
        assert!(
            matches!(
                Job::open(&out, name, std::slice::from_ref(&source), false),
                Err(Error::Usage(_))
            ),
            "{name:?}"
        );
    }
    assert!(!out.exists(), "a rejected name must not create anything");
}

#[test]
fn fingerprint_is_stable_and_separates_parts() {
    let a = fingerprint(&[b"ab", b"c"]);
    assert_eq!(a, fingerprint(&[b"ab", b"c"]));
    assert_eq!(a.len(), 64);
    assert_ne!(a, fingerprint(&[b"a", b"bc"]));
    assert_ne!(a, fingerprint(&[b"ab", b"d"]));
}

#[test]
fn write_atomic_failure_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let fresh = dir.path().join("audio.wav");
    let result = write_atomic(&fresh, |tmp| {
        fs::write(tmp, b"partial")?;
        Err(Error::Interrupted)
    });
    assert!(matches!(result, Err(Error::Interrupted)));
    assert!(!fresh.exists());
    assert!(!dir.path().join("audio.wav.tmp").exists());

    let kept = dir.path().join("transcript.json");
    fs::write(&kept, b"good").unwrap();
    let result = write_atomic(&kept, |tmp| {
        fs::write(tmp, b"partial")?;
        Err(Error::Interrupted)
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&kept).unwrap(), b"good");
    assert!(!dir.path().join("transcript.json.tmp").exists());
}

#[test]
fn write_atomic_success_renames_tmp() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("transcript.json");
    fs::write(&dest, b"old").unwrap();
    write_atomic(&dest, |tmp| {
        assert_eq!(tmp, dir.path().join("transcript.json.tmp"));
        fs::write(tmp, b"new")?;
        Ok(())
    })
    .unwrap();
    assert_eq!(fs::read(&dest).unwrap(), b"new");
    assert!(!dir.path().join("transcript.json.tmp").exists());
}

#[test]
fn step_names_parse_in_pipeline_order() {
    let names: Vec<StepName> = ["probe", "join", "audio", "transcribe", "outputs"]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(names, StepName::ALL);
    assert!(names.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(StepName::Transcribe.to_string(), "transcribe");
    assert!(matches!("render".parse::<StepName>(), Err(Error::Usage(_))));
}

#[test]
fn job_json_has_version_1() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    assert_eq!(job.dir, fs::canonicalize(out.join("clase-4")).unwrap());
    let json = read_json(&job.dir.join("job.json"));
    assert_eq!(json["version"], 1);
    assert_eq!(json["name"], "clase-4");
    let recorded = &json["sources"][0];
    assert_eq!(
        Path::new(recorded["path"].as_str().unwrap()),
        fs::canonicalize(&source).unwrap()
    );
    assert_eq!(recorded["size"], 12);
    assert!(recorded["mtime"].as_u64().is_some());
    assert_eq!(json["steps"], serde_json::json!({}));
}

#[test]
fn unknown_version_rejected() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let file = job.dir.join("job.json");
    let mut json = read_json(&file);
    json["version"] = 2.into();
    let written = serde_json::to_vec(&json).unwrap();
    fs::write(&file, &written).unwrap();

    match Job::open(&out, "clase-4", &[source], false) {
        Err(Error::Job(msg)) => assert!(msg.contains("version 2"), "{msg}"),
        other => panic!("expected Error::Job, got {other:?}"),
    }
    assert_eq!(
        fs::read(&file).unwrap(),
        written,
        "job.json must not be migrated"
    );
}

#[test]
fn tmp_cleaned_on_open() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    fs::write(job.dir.join("audio.wav.tmp"), b"partial").unwrap();
    fs::write(job.dir.join("job.json.tmp"), b"partial").unwrap();
    fs::write(job.dir.join("audio.wav"), b"good").unwrap();

    let job = Job::open(&out, "clase-4", &[source], false).unwrap();
    assert!(!job.dir.join("audio.wav.tmp").exists());
    assert!(!job.dir.join("job.json.tmp").exists());
    assert!(job.dir.join("audio.wav").exists());
}

#[test]
fn tmp_cleanup_spares_other_names() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    fs::create_dir(job.dir.join("cache.tmp")).unwrap();
    fs::write(job.dir.join("notes.tmpl"), b"keep").unwrap();

    let job = Job::open(&out, "clase-4", &[source], false).unwrap();
    assert!(job.dir.join("cache.tmp").is_dir());
    assert!(job.dir.join("notes.tmpl").exists());
}

#[test]
fn missing_version_rejected() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let file = job.dir.join("job.json");
    let mut json = read_json(&file);
    json.as_object_mut().unwrap().remove("version");
    let written = serde_json::to_vec(&json).unwrap();
    fs::write(&file, &written).unwrap();

    assert!(matches!(
        Job::open(&out, "clase-4", &[source], false),
        Err(Error::Job(_))
    ));
    assert_eq!(fs::read(&file).unwrap(), written);
}

#[test]
fn invalid_job_json_rejected() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let file = job.dir.join("job.json");
    fs::write(&file, b"{ not json").unwrap();

    match Job::open(&out, "clase-4", &[source], false) {
        Err(Error::Job(msg)) => assert!(msg.contains("job.json"), "{msg}"),
        other => panic!("expected Error::Job, got {other:?}"),
    }
    assert_eq!(fs::read(&file).unwrap(), b"{ not json");
}

#[test]
fn failed_open_touches_nothing() {
    let (root, out, source) = setup();
    let other = root.path().join("other.mp4");
    fs::write(&other, b"other bytes").unwrap();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let leftover = job.dir.join("audio.wav.tmp");
    fs::write(&leftover, b"another run").unwrap();
    let before = fs::read(job.dir.join("job.json")).unwrap();

    assert!(Job::open(&out, "clase-4", &[other], false).is_err());
    assert!(leftover.exists(), "a refused open must not clean tmp files");
    assert_eq!(fs::read(job.dir.join("job.json")).unwrap(), before);

    let missing = root.path().join("missing.mp4");
    assert!(Job::open(&out, "missing", &[missing], false).is_err());
    assert!(!out.join("missing").exists());
    assert!(leftover.exists());
}

#[test]
fn relative_out_dir_records_artifacts() {
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let cwd = std::env::current_dir().unwrap();
    let out = tmp.path().strip_prefix(&cwd).unwrap().join("minuta-out");
    assert!(out.is_relative());
    let source = tmp.path().join("clase-4.mp4");
    fs::write(&source, b"source bytes").unwrap();

    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    assert!(job.dir.is_absolute());
    let ctx = job.ctx();
    let artifact = ctx.dir.join("audio.wav");
    fs::write(&artifact, b"wav").unwrap();
    job.mark_done(StepName::Audio, "fp", 0.1, &[artifact])
        .unwrap();

    assert_eq!(
        job.state.steps[&StepName::Audio].artifacts,
        vec![PathBuf::from("audio.wav")]
    );
    let current = vec![(StepName::Audio, "fp".to_string())];
    assert_eq!(plan_stale(&job, &current, None), vec![]);
}

#[test]
fn name_conflict_different_sources() {
    let (root, out, source) = setup();
    let other = root.path().join("sub").join("clase-4.mp4");
    fs::create_dir_all(other.parent().unwrap()).unwrap();
    fs::write(&other, b"other bytes").unwrap();

    let mut job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    job.mark_done(StepName::Probe, "fp", 0.1, &[]).unwrap();

    match Job::open(&out, "clase-4", std::slice::from_ref(&other), false) {
        Err(Error::Job(msg)) => assert!(msg.contains("different sources"), "{msg}"),
        other => panic!("expected Error::Job, got {other:?}"),
    }
    match Job::open(&out, "clase-4", &[source.clone(), other.clone()], false) {
        Err(Error::Job(_)) => {}
        other => panic!("expected Error::Job, got {other:?}"),
    }
    match Job::open(&out, "clase-4", &[other.clone(), source.clone()], false) {
        Err(Error::Job(_)) => {}
        other => panic!("expected Error::Job, got {other:?}"),
    }
    let mut both = Job::open(&out, "both", &[source.clone(), other.clone()], false).unwrap();
    both.mark_done(StepName::Probe, "fp", 0.1, &[]).unwrap();
    match Job::open(&out, "both", &[other.clone(), source.clone()], false) {
        Err(Error::Job(msg)) => assert!(msg.contains("different sources"), "{msg}"),
        other => panic!("source order must count, got {other:?}"),
    }
    let same = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    assert_eq!(same.state.steps.len(), 1, "same sources keep the job");

    let replaced = Job::open(&out, "clase-4", std::slice::from_ref(&other), true).unwrap();
    assert_eq!(replaced.state.sources.len(), 1);
    assert_eq!(
        replaced.state.sources[0].path,
        fs::canonicalize(&other).unwrap()
    );
    assert!(
        replaced.state.steps.is_empty(),
        "replacing starts the job over"
    );
}

#[test]
fn fresh_job_steps_not_run() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", &[source], false).unwrap();
    let plan = plan_stale(&job, &current(), None);
    let expected: Vec<_> = StepName::ALL.iter().map(|&s| (s, Reason::NotRun)).collect();
    assert_eq!(plan, expected);
}

#[test]
fn changed_fingerprint_marks_stale() {
    let (_root, mut job) = done_job();
    let mut fps = current();
    fps[2].1 = "fp-audio-new".into();
    let plan = plan_stale(&job, &fps, None);
    assert_eq!(
        plan,
        stale_from(StepName::Audio, Reason::FingerprintChanged)
    );

    job.mark_stale(&plan).unwrap();
    let json = read_json(&job.dir.join("job.json"));
    assert_eq!(json["steps"]["audio"]["status"], "stale");
    assert_eq!(json["steps"]["audio"]["reason"], "fingerprint changed");
    assert_eq!(
        json["steps"]["outputs"]["reason"],
        "earlier step audio is stale"
    );
    assert_eq!(json["steps"]["join"]["status"], "done");
    assert_eq!(json["steps"]["join"]["reason"], serde_json::Value::Null);
}

#[test]
fn stale_propagates_to_later_steps() {
    let (_root, job) = done_job();
    let mut fps = current();
    fps[0].1 = "fp-probe-new".into();
    let plan = plan_stale(&job, &fps, None);
    assert_eq!(
        plan,
        stale_from(StepName::Probe, Reason::FingerprintChanged)
    );
}

#[test]
fn interrupted_step_stays_stale() {
    let (_root, mut job) = done_job();
    job.mark_stale(&stale_from(StepName::Transcribe, Reason::Forced))
        .unwrap();
    let artifact = job.dir.join("transcribe.out");
    job.mark_done(StepName::Transcribe, "fp-transcribe", 0.1, &[artifact])
        .unwrap();
    // Interrupted before `outputs` finished: its fingerprint and artifact still match.
    let plan = plan_stale(&job, &current(), None);
    assert_eq!(plan, vec![(StepName::Outputs, Reason::Unfinished)]);
}

#[test]
fn missing_artifact_marks_stale() {
    let (_root, job) = done_job();
    fs::remove_file(job.dir.join("join.out")).unwrap();
    let plan = plan_stale(&job, &current(), None);
    assert_eq!(
        plan,
        stale_from(StepName::Join, Reason::MissingArtifact("join.out".into()))
    );
}

#[test]
fn force_all() {
    let (_root, job) = done_job();
    let plan = plan_stale(&job, &current(), Some(Force::All));
    let expected: Vec<_> = StepName::ALL.iter().map(|&s| (s, Reason::Forced)).collect();
    assert_eq!(plan, expected);
}

#[test]
fn force_step_and_later() {
    let (_root, job) = done_job();
    let plan = plan_stale(&job, &current(), Some(Force::From(StepName::Transcribe)));
    assert_eq!(
        plan,
        vec![
            (StepName::Transcribe, Reason::Forced),
            (StepName::Outputs, Reason::Forced),
        ]
    );
}

#[test]
fn done_requires_all_artifacts() {
    let (_root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    let present = job.dir.join("audio.wav");
    fs::write(&present, b"wav").unwrap();
    let missing = job.dir.join("audio.json");

    match job.mark_done(StepName::Audio, "fp", 0.1, &[present.clone(), missing]) {
        Err(Error::Job(msg)) => assert!(msg.contains("audio.json"), "{msg}"),
        other => panic!("expected Error::Job, got {other:?}"),
    }
    assert!(!job.state.steps.contains_key(&StepName::Audio));
    let json = read_json(&job.dir.join("job.json"));
    assert_eq!(json["steps"]["audio"], serde_json::Value::Null);

    job.mark_done(StepName::Audio, "fp", 0.1, &[present])
        .unwrap();
    let json = read_json(&job.dir.join("job.json"));
    assert_eq!(json["steps"]["audio"]["status"], "done");
    assert_eq!(
        json["steps"]["audio"]["artifacts"],
        serde_json::json!(["audio.wav"])
    );
    assert_eq!(job.state.steps[&StepName::Audio].status, Status::Done);
}

/// A step that writes nothing; `I` indexes `StepName::ALL`, `P` is its parameter byte.
struct Fake<const I: usize, const P: u8 = 0>;

impl<const I: usize, const P: u8> Step for Fake<I, P> {
    const NAME: StepName = StepName::ALL[I];

    fn params(&self, _ctx: &Ctx) -> Vec<u8> {
        vec![P]
    }

    fn outputs(&self, _ctx: &Ctx) -> Vec<PathBuf> {
        Vec::new()
    }

    fn run(&self, _ctx: &Ctx) -> minuta::error::Result<()> {
        Ok(())
    }
}

fn push<S: Step>(step: &S, ctx: &Ctx, fps: &mut Vec<(StepName, String)>) {
    let previous = fps.last().map(|(_, fp)| fp.as_str());
    let fp = step_fingerprint(step, ctx, previous);
    fps.push((S::NAME, fp));
}

/// Chained fingerprints of the five fake steps for `job`.
fn chained(job: &Job) -> Vec<(StepName, String)> {
    let ctx = job.ctx();
    let mut fps = Vec::new();
    push(&Fake::<0>, &ctx, &mut fps);
    push(&Fake::<1>, &ctx, &mut fps);
    push(&Fake::<2>, &ctx, &mut fps);
    push(&Fake::<3>, &ctx, &mut fps);
    push(&Fake::<4>, &ctx, &mut fps);
    fps
}

/// Opens a job for `source` and records every fake step as done.
fn done_chained_job(out: &Path, source: &Path) -> Job {
    let mut job = Job::open(out, "clase-4", &[source.to_path_buf()], false).unwrap();
    for (step, fp) in chained(&job) {
        job.mark_done(step, &fp, 0.1, &[]).unwrap();
    }
    job
}

fn set_mtime(path: &Path, offset: Duration) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() + offset).unwrap();
}

#[test]
fn ctx_describes_the_job() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let ctx = job.ctx();
    assert_eq!(ctx.dir, job.dir);
    assert_eq!(ctx.sources, vec![fs::canonicalize(&source).unwrap()]);
    assert_eq!(ctx.source_digest.len(), 64);
}

#[test]
fn step_fingerprint_covers_params_sources_and_previous() {
    let (root, out, source) = setup();
    let ctx = Job::open(&out, "clase-4", &[source], false).unwrap().ctx();
    let base = step_fingerprint(&Fake::<3, 0>, &ctx, Some("prev"));
    assert_eq!(base, step_fingerprint(&Fake::<3, 0>, &ctx, Some("prev")));
    assert_ne!(base, step_fingerprint(&Fake::<3, 1>, &ctx, Some("prev")));
    assert_ne!(base, step_fingerprint(&Fake::<3, 0>, &ctx, Some("other")));
    assert_ne!(base, step_fingerprint(&Fake::<3, 0>, &ctx, None));
    assert_ne!(base, step_fingerprint(&Fake::<4, 0>, &ctx, Some("prev")));

    let other = root.path().join("other.mp4");
    fs::write(&other, b"other bytes").unwrap();
    let other_ctx = Job::open(&out, "other", &[other], false).unwrap().ctx();
    assert_ne!(
        base,
        step_fingerprint(&Fake::<3, 0>, &other_ctx, Some("prev"))
    );
}

#[test]
fn size_and_mtime_match_skips_hash() {
    let (_root, _out, source) = setup();
    let real = source_record(&source, None).unwrap();
    let previous = Source {
        hash: "recorded".into(),
        ..real.clone()
    };
    let reused = source_record(&source, Some(&previous)).unwrap();
    assert_eq!(
        reused.hash, "recorded",
        "matching size and mtime must not rehash"
    );

    let moved = Source {
        mtime: previous.mtime + 1,
        ..previous.clone()
    };
    assert_eq!(
        source_record(&source, Some(&moved)).unwrap().hash,
        real.hash
    );
    let resized = Source {
        size: previous.size + 1,
        ..previous
    };
    assert_eq!(
        source_record(&source, Some(&resized)).unwrap().hash,
        real.hash
    );
}

#[test]
fn mtime_change_same_content_stays_cached() {
    let (_root, out, source) = setup();
    let before = done_chained_job(&out, &source).state.sources[0].clone();
    set_mtime(&source, Duration::from_secs(3600));

    let job = Job::open(&out, "clase-4", &[source], false).unwrap();
    assert_eq!(plan_stale(&job, &chained(&job), None), vec![]);
    let after = &job.state.sources[0];
    assert_ne!(after.mtime, before.mtime, "the new mtime is recorded");
    assert_eq!(after.hash, before.hash);
}

#[test]
fn changed_source_content_marks_stale() {
    let (_root, out, source) = setup();
    done_chained_job(&out, &source);
    fs::write(&source, b"SOURCE BYTES").unwrap();
    set_mtime(&source, Duration::from_secs(3600));

    let job = Job::open(&out, "clase-4", &[source], false).unwrap();
    let plan = plan_stale(&job, &chained(&job), None);
    let expected: Vec<_> = StepName::ALL
        .iter()
        .map(|&s| (s, Reason::FingerprintChanged))
        .collect();
    assert_eq!(plan, expected);
}

#[test]
fn mtime_before_epoch_always_rehashes() {
    let (_root, _out, source) = setup();
    let file = fs::File::options().write(true).open(&source).unwrap();
    file.set_modified(SystemTime::UNIX_EPOCH - Duration::from_secs(10))
        .unwrap();
    let real = source_record(&source, None).unwrap();
    let previous = Source {
        hash: "recorded".into(),
        ..real.clone()
    };
    let again = source_record(&source, Some(&previous)).unwrap();
    assert_eq!(
        again.hash, real.hash,
        "an unknown mtime must not trust the cache"
    );
}

/// A job where template `summary` is done with fingerprint `fp-summary` and artifact `summary.md`.
fn done_template_job() -> (TempDir, Job) {
    let (root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    fs::write(job.dir.join("summary.md"), "notes").unwrap();
    job.mark_template_done("summary", "fp-summary", 1.5, &[PathBuf::from("summary.md")])
        .unwrap();
    (root, job)
}

#[test]
fn old_job_json_loads_without_templates() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let file = job.dir.join("job.json");
    let mut json = read_json(&file);
    json.as_object_mut().unwrap().remove("templates");
    fs::write(&file, serde_json::to_vec(&json).unwrap()).unwrap();
    let job = Job::open(&out, "clase-4", &[source], false).unwrap();
    assert!(job.state.templates.is_empty());
    let saved = read_json(&file);
    assert_eq!(saved["templates"], serde_json::json!({}));
    assert_eq!(saved["version"], 1);
}

#[test]
fn template_not_run() {
    let (_root, job) = done_template_job();
    assert_eq!(
        template_reason(&job, "other", "fp", false).unwrap(),
        Some(Reason::NotRun)
    );
}

#[test]
fn template_unfinished() {
    let (_root, mut job) = done_template_job();
    job.mark_template_stale("summary", &Reason::FingerprintChanged)
        .unwrap();
    assert_eq!(
        template_reason(&job, "summary", "fp-summary", false).unwrap(),
        Some(Reason::Unfinished)
    );
}

#[test]
fn template_forced() {
    let (_root, job) = done_template_job();
    assert_eq!(
        template_reason(&job, "summary", "fp-summary", true).unwrap(),
        Some(Reason::Forced)
    );
}

#[test]
fn template_fingerprint_changed() {
    let (_root, job) = done_template_job();
    assert_eq!(
        template_reason(&job, "summary", "other", false).unwrap(),
        Some(Reason::FingerprintChanged)
    );
}

#[test]
fn template_missing_artifact() {
    let (_root, job) = done_template_job();
    fs::remove_file(job.dir.join("summary.md")).unwrap();
    assert_eq!(
        template_reason(&job, "summary", "fp-summary", false).unwrap(),
        Some(Reason::MissingArtifact("summary.md".to_owned()))
    );
}

#[test]
fn template_current_is_none() {
    let (_root, job) = done_template_job();
    assert_eq!(
        template_reason(&job, "summary", "fp-summary", false).unwrap(),
        None
    );
}

#[test]
fn mark_template_done_requires_artifacts() {
    let (_root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    let err = job
        .mark_template_done("summary", "fp", 1.0, &[PathBuf::from("summary.md")])
        .unwrap_err();
    assert!(matches!(err, Error::Job(_)));
    assert!(job.state.templates.is_empty());
    assert!(read_json(&job.dir.join("job.json"))["templates"]
        .as_object()
        .unwrap()
        .is_empty());

    let absolute = job.dir.join("summary.md");
    fs::write(&absolute, "notes").unwrap();
    job.mark_template_done("summary", "fp", 1.0, &[absolute])
        .unwrap();
    assert_eq!(
        read_json(&job.dir.join("job.json"))["templates"]["summary"]["artifacts"],
        serde_json::json!(["summary.md"])
    );
}

#[test]
fn mark_template_stale_records_reason() {
    let (_root, mut job) = done_template_job();
    job.mark_template_stale("summary", &Reason::Forced).unwrap();
    let saved = read_json(&job.dir.join("job.json"));
    assert_eq!(saved["templates"]["summary"]["status"], "stale");
    assert_eq!(saved["templates"]["summary"]["reason"], "forced");
    job.mark_template_stale("new", &Reason::NotRun).unwrap();
    assert_eq!(job.state.templates["new"].status, Status::Stale);
}
