use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use minuta::error::Error;
use minuta::job::{
    derive_name, fingerprint, layout, plan_stale, source_record, step_fingerprint, template_reason,
    write_atomic, write_atomic_in, Ctx, Force, Job, Reason, Source, Status, Step, StepName,
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

#[cfg(unix)]
#[test]
fn write_atomic_does_not_follow_a_planted_tmp_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let victim = dir.path().join("victim.txt");
    fs::write(&victim, b"precious").unwrap();
    let dest = dir.path().join("summary.md");
    fs::write(&dest, b"old").unwrap();
    std::os::unix::fs::symlink(&victim, dir.path().join("summary.md.tmp")).unwrap();

    write_atomic(&dest, |tmp| {
        fs::write(tmp, b"new")?;
        Ok(())
    })
    .unwrap();

    assert_eq!(fs::read(&victim).unwrap(), b"precious");
    assert_eq!(fs::read(&dest).unwrap(), b"new");
    assert!(!dir.path().join("summary.md.tmp").exists());
    assert!(!dir.path().join("summary.md.tmp").is_symlink());
}

#[cfg(unix)]
#[test]
fn write_atomic_does_not_follow_a_dangling_tmp_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("not-yet.txt");
    let dest = dir.path().join("summary.md");
    std::os::unix::fs::symlink(&missing, dir.path().join("summary.md.tmp")).unwrap();

    write_atomic(&dest, |tmp| {
        fs::write(tmp, b"new")?;
        Ok(())
    })
    .unwrap();

    assert!(!missing.exists());
    assert_eq!(fs::read(&dest).unwrap(), b"new");
}

#[test]
fn write_atomic_replaces_a_stale_regular_tmp() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("summary.md");
    fs::write(dir.path().join("summary.md.tmp"), b"stale partial output").unwrap();

    write_atomic(&dest, |tmp| {
        assert!(!tmp.exists(), "the callback must find no temp entry");
        fs::write(tmp, b"new")?;
        Ok(())
    })
    .unwrap();

    assert_eq!(fs::read(&dest).unwrap(), b"new");
    assert!(!dir.path().join("summary.md.tmp").exists());
}

#[test]
fn write_atomic_reports_an_unremovable_tmp_by_path() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("summary.md");
    let tmp = dir.path().join("summary.md.tmp");
    fs::create_dir(&tmp).unwrap();
    fs::write(tmp.join("inside"), b"x").unwrap();

    let mut called = false;
    let err = write_atomic(&dest, |_| {
        called = true;
        Ok(())
    })
    .unwrap_err();

    assert!(!called, "the callback must not run");
    assert!(matches!(err, Error::Io(_)), "{err:?}");
    assert!(
        err.to_string().contains(&tmp.display().to_string()),
        "{err}"
    );
    assert!(tmp.join("inside").exists());
    assert!(!dest.exists());
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
    let json = read_json(&layout::state_file(&job.dir));
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
    let file = layout::state_file(&job.dir);
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
    fs::write(job.dir.join(".minuta").join("job.json.tmp"), b"partial").unwrap();
    fs::write(job.dir.join("audio.wav"), b"good").unwrap();

    let job = Job::open(&out, "clase-4", &[source], false).unwrap();
    assert!(!job.dir.join("audio.wav.tmp").exists());
    assert!(!job.dir.join(".minuta").join("job.json.tmp").exists());
    assert!(job.dir.join("audio.wav").exists());
}

#[cfg(unix)]
#[test]
fn tmp_cleanup_removes_symlinks_without_following_them() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let victim_dir = tempfile::tempdir().unwrap();
    let victim = victim_dir.path().join("victim.txt");
    fs::write(&victim, b"precious").unwrap();
    let victim_subdir = victim_dir.path().join("subdir");
    fs::create_dir(&victim_subdir).unwrap();
    fs::write(victim_subdir.join("inside"), b"keep").unwrap();
    let dangling = victim_dir.path().join("missing");
    std::os::unix::fs::symlink(&victim, job.dir.join("summary.md.tmp")).unwrap();
    std::os::unix::fs::symlink(&victim_subdir, job.dir.join("dir.tmp")).unwrap();
    std::os::unix::fs::symlink(&dangling, job.dir.join("dangling.tmp")).unwrap();
    fs::write(job.dir.join("audio.wav.tmp"), b"stale").unwrap();
    fs::write(job.dir.join("summary.md"), b"good").unwrap();
    std::os::unix::fs::symlink(&victim, job.dir.join("link-not-tmp")).unwrap();

    job.clean_tmp().unwrap();

    for name in ["summary.md.tmp", "dir.tmp", "dangling.tmp", "audio.wav.tmp"] {
        assert!(
            fs::symlink_metadata(job.dir.join(name)).is_err(),
            "{name} should be gone"
        );
    }
    assert_eq!(fs::read(&victim).unwrap(), b"precious");
    assert_eq!(fs::read(victim_subdir.join("inside")).unwrap(), b"keep");
    assert_eq!(fs::read(job.dir.join("summary.md")).unwrap(), b"good");
    assert!(job.dir.join("link-not-tmp").is_symlink());
}

#[cfg(unix)]
#[test]
fn tmp_cleanup_reports_an_unremovable_entry_by_path() {
    use std::os::unix::fs::PermissionsExt;

    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", &[source], false).unwrap();
    let planted = job.dir.join("summary.md.tmp");
    std::os::unix::fs::symlink("victim", &planted).unwrap();
    fs::set_permissions(&job.dir, fs::Permissions::from_mode(0o555)).unwrap();
    let result = job.clean_tmp();
    // Running as root: the permission bits do not stop the removal, so there is no error.
    let enforced = fs::File::create(job.dir.join("probe-writable")).is_err();
    fs::set_permissions(&job.dir, fs::Permissions::from_mode(0o755)).unwrap();
    if !enforced {
        return;
    }
    let err = result.unwrap_err();
    assert!(matches!(err, Error::Io(_)), "{err:?}");
    assert!(
        err.to_string().contains(&planted.display().to_string()),
        "{err}"
    );
    assert!(planted.is_symlink());
}

#[cfg(unix)]
#[test]
fn tmp_cleaned_on_open_removes_a_planted_symlink() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let victim_dir = tempfile::tempdir().unwrap();
    let victim = victim_dir.path().join("victim.txt");
    fs::write(&victim, b"precious").unwrap();
    std::os::unix::fs::symlink(&victim, job.dir.join("summary.md.tmp")).unwrap();

    Job::open(&out, "clase-4", &[source], false).unwrap();

    assert!(fs::symlink_metadata(job.dir.join("summary.md.tmp")).is_err());
    assert_eq!(fs::read(&victim).unwrap(), b"precious");
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
    let file = layout::state_file(&job.dir);
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
    let file = layout::state_file(&job.dir);
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
    let before = fs::read(layout::state_file(&job.dir)).unwrap();

    assert!(Job::open(&out, "clase-4", &[other], false).is_err());
    assert!(leftover.exists(), "a refused open must not clean tmp files");
    assert_eq!(fs::read(layout::state_file(&job.dir)).unwrap(), before);

    let missing = root.path().join("missing.mp4");
    assert!(Job::open(&out, "missing", &[missing], false).is_err());
    assert!(!out.join("missing").exists());
    assert!(leftover.exists());
}

/// `path` relative to `base`, climbing with `..` where needed. Both must be absolute and
/// `base` must hold no symlinks, so `..` from it climbs the spelled-out components.
fn relative_to(path: &Path, base: &Path) -> PathBuf {
    let path: Vec<_> = path.components().collect();
    let base: Vec<_> = base.components().collect();
    let common = path.iter().zip(&base).take_while(|(a, b)| a == b).count();
    let mut relative = PathBuf::new();
    for _ in common..base.len() {
        relative.push("..");
    }
    for component in &path[common..] {
        relative.push(component);
    }
    relative
}

#[test]
fn relative_out_dir_records_artifacts() {
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let cwd = fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
    let out = relative_to(&fs::canonicalize(tmp.path()).unwrap(), &cwd).join("minuta-out");
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
    let json = read_json(&layout::state_file(&job.dir));
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
    let json = read_json(&layout::state_file(&job.dir));
    assert_eq!(json["steps"]["audio"], serde_json::Value::Null);

    job.mark_done(StepName::Audio, "fp", 0.1, &[present])
        .unwrap();
    let json = read_json(&layout::state_file(&job.dir));
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
    let file = layout::state_file(&job.dir);
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
    assert!(read_json(&layout::state_file(&job.dir))["templates"]
        .as_object()
        .unwrap()
        .is_empty());

    let absolute = job.dir.join("summary.md");
    fs::write(&absolute, "notes").unwrap();
    job.mark_template_done("summary", "fp", 1.0, &[absolute])
        .unwrap();
    assert_eq!(
        read_json(&layout::state_file(&job.dir))["templates"]["summary"]["artifacts"],
        serde_json::json!(["summary.md"])
    );
}

#[test]
fn mark_template_stale_records_reason() {
    let (_root, mut job) = done_template_job();
    job.mark_template_stale("summary", &Reason::Forced).unwrap();
    let saved = read_json(&layout::state_file(&job.dir));
    assert_eq!(saved["templates"]["summary"]["status"], "stale");
    assert_eq!(saved["templates"]["summary"]["reason"], "forced");
    job.mark_template_stale("new", &Reason::NotRun).unwrap();
    assert_eq!(job.state.templates["new"].status, Status::Stale);
}

#[test]
fn template_reason_surfaces_io_error() {
    let (_root, mut job) = done_template_job();
    // `summary.md` is a regular file, so checking `summary.md/child` fails with NotADirectory.
    job.state.templates.get_mut("summary").unwrap().artifacts =
        vec![PathBuf::from("summary.md/child")];
    let err = template_reason(&job, "summary", "fp-summary", false).unwrap_err();
    assert!(err.to_string().contains("summary.md/child"), "{err}");
}

#[test]
fn mark_template_done_surfaces_io_error() {
    let (_root, mut job) = done_template_job();
    let err = job
        .mark_template_done("other", "fp", 1.0, &[PathBuf::from("summary.md/child")])
        .unwrap_err();
    assert!(err.to_string().contains("summary.md/child"), "{err}");
    assert!(!job.state.templates.contains_key("other"));
}

#[test]
fn derive_name_rejects_bare_mp4() {
    let err = derive_name(Path::new(".mp4")).unwrap_err();
    assert_eq!(
        err.to_string(),
        "usage error: cannot derive a job name from `.mp4`: \
         the file name is only a media extension"
    );
}

#[test]
fn derive_name_rejects_bare_mkv() {
    let err = derive_name(Path::new("/videos/.mkv")).unwrap_err();
    assert_eq!(
        err.to_string(),
        "usage error: cannot derive a job name from `/videos/.mkv`: \
         the file name is only a media extension"
    );
}

#[test]
fn derive_name_rejects_stacked_bare_extensions() {
    let err = derive_name(Path::new(".mp4.wav")).unwrap_err();
    assert_eq!(
        err.to_string(),
        "usage error: cannot derive a job name from `.mp4.wav`: \
         the file name is only a media extension"
    );
}

#[test]
fn derive_name_keeps_other_dotfiles() {
    assert_eq!(derive_name(Path::new(".notes")).unwrap(), ".notes");
}

#[test]
fn mark_done_surfaces_io_error() {
    let (_root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    fs::write(job.dir.join("audio.wav"), b"wav").unwrap();
    // `audio.wav` is a regular file, so checking `audio.wav/child` fails with NotADirectory.
    let err = job
        .mark_done(
            StepName::Audio,
            "fp",
            0.1,
            &[PathBuf::from("audio.wav/child")],
        )
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "job error: cannot check {}: Not a directory (os error 20)",
            job.dir.join("audio.wav/child").display()
        )
    );
    assert!(!job.state.steps.contains_key(&StepName::Audio));
}

#[test]
fn mark_done_missing_artifact_names_step() {
    let (_root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    let err = job
        .mark_done(StepName::Audio, "fp", 0.1, &[PathBuf::from("audio.wav")])
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "job error: step audio finished without writing audio.wav"
    );
}

#[test]
fn mark_template_done_missing_artifact_names_template() {
    let (_root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    let err = job
        .mark_template_done("summary", "fp", 1.0, &[PathBuf::from("summary.md")])
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "job error: template summary finished without writing summary.md"
    );
}

#[test]
fn mark_done_rejects_artifact_outside_job_dir() {
    let (root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    let outside = root.path().join("outside.wav");
    fs::write(&outside, b"wav").unwrap();
    let err = job
        .mark_done(StepName::Audio, "fp", 0.1, std::slice::from_ref(&outside))
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "job error: step audio wrote {} outside the job folder {}",
            outside.display(),
            job.dir.display()
        )
    );
    assert!(!job.state.steps.contains_key(&StepName::Audio));
    assert_eq!(
        read_json(&layout::state_file(&job.dir))["steps"],
        serde_json::json!({})
    );
}

#[test]
fn mark_done_rejects_relative_artifact_escaping_job_dir() {
    let (root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    fs::write(root.path().join("outside.wav"), b"wav").unwrap();
    let escaping = PathBuf::from("../../outside.wav");
    let err = job
        .mark_done(StepName::Audio, "fp", 0.1, std::slice::from_ref(&escaping))
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "job error: step audio wrote ../../outside.wav outside the job folder {}",
            job.dir.display()
        )
    );
}

#[test]
fn mark_template_done_rejects_artifact_outside_job_dir() {
    let (root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    let outside = root.path().join("summary.md");
    fs::write(&outside, b"notes").unwrap();
    let err = job
        .mark_template_done("summary", "fp", 1.0, std::slice::from_ref(&outside))
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "job error: template summary wrote {} outside the job folder {}",
            outside.display(),
            job.dir.display()
        )
    );
    assert!(job.state.templates.is_empty());
}

#[test]
fn mark_done_relativizes_non_canonical_inside_path() {
    let (_root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    fs::write(job.dir.join("audio.wav"), b"wav").unwrap();
    // `out` is not canonical on every platform (macOS temp dirs sit behind `/var`), and the
    // `..` detour makes this spelling differ from `job.dir` everywhere.
    let spelled = out
        .join("clase-4")
        .join("..")
        .join("clase-4")
        .join("audio.wav");
    job.mark_done(StepName::Audio, "fp", 0.1, &[spelled])
        .unwrap();
    assert_eq!(
        job.state.steps[&StepName::Audio].artifacts,
        vec![PathBuf::from("audio.wav")]
    );
}

#[test]
fn mark_done_relativizes_nested_relative_path() {
    let (_root, out, source) = setup();
    let mut job = Job::open(&out, "clase-4", &[source], false).unwrap();
    fs::create_dir(job.dir.join("parts")).unwrap();
    fs::write(job.dir.join("parts").join("a.wav"), b"wav").unwrap();
    job.mark_done(StepName::Audio, "fp", 0.1, &[PathBuf::from("parts/a.wav")])
        .unwrap();
    assert_eq!(
        job.state.steps[&StepName::Audio].artifacts,
        vec![PathBuf::from("parts/a.wav")]
    );
}

/// Builds a job with step `audio` and template `summary` done, lets `edit` change its saved
/// `job.json`, and returns the error of reopening it with the `job.json` path `Job::open` reads.
fn reopen_after_edit(edit: impl FnOnce(&mut serde_json::Value)) -> (String, PathBuf) {
    let (root, mut job) = done_template_job();
    let artifact = job.dir.join("audio.wav");
    fs::write(&artifact, b"wav").unwrap();
    job.mark_done(StepName::Audio, "fp-audio", 0.1, &[artifact])
        .unwrap();
    let file = layout::state_file(&job.dir);
    let mut json = read_json(&file);
    edit(&mut json);
    fs::write(&file, serde_json::to_vec(&json).unwrap()).unwrap();

    let out = root.path().join("minuta-out");
    let source = root.path().join("clase-4.mp4");
    let err = Job::open(&out, "clase-4", &[source], false).unwrap_err();
    (err.to_string(), layout::state_file(&out.join("clase-4")))
}

/// Asserts that reopening after `edit` fails with exactly `invalid <job.json>: <problem>; move
/// or delete it`.
fn assert_state_rejected(edit: impl FnOnce(&mut serde_json::Value), problem: &str) {
    let (err, file) = reopen_after_edit(edit);
    assert_eq!(
        err,
        format!(
            "job error: invalid {}: {problem}; move or delete it",
            file.display()
        )
    );
}

#[test]
fn step_done_without_fingerprint_rejected() {
    assert_state_rejected(
        |json| json["steps"]["audio"]["fingerprint"] = serde_json::Value::Null,
        "step audio is done without a fingerprint",
    );
}

#[test]
fn step_done_without_seconds_rejected() {
    assert_state_rejected(
        |json| json["steps"]["audio"]["seconds"] = serde_json::Value::Null,
        "step audio is done without seconds",
    );
}

#[test]
fn step_done_with_reason_rejected() {
    assert_state_rejected(
        |json| json["steps"]["audio"]["reason"] = "forced".into(),
        "step audio is done but has a stale reason",
    );
}

#[test]
fn step_stale_without_reason_rejected() {
    assert_state_rejected(
        |json| json["steps"]["audio"]["status"] = "stale".into(),
        "step audio is stale without a reason",
    );
}

#[test]
fn template_done_without_fingerprint_rejected() {
    assert_state_rejected(
        |json| json["templates"]["summary"]["fingerprint"] = serde_json::Value::Null,
        "template summary is done without a fingerprint",
    );
}

#[test]
fn template_done_without_seconds_rejected() {
    assert_state_rejected(
        |json| json["templates"]["summary"]["seconds"] = serde_json::Value::Null,
        "template summary is done without seconds",
    );
}

#[test]
fn template_done_with_reason_rejected() {
    assert_state_rejected(
        |json| json["templates"]["summary"]["reason"] = "forced".into(),
        "template summary is done but has a stale reason",
    );
}

#[test]
fn template_stale_without_reason_rejected() {
    assert_state_rejected(
        |json| json["templates"]["summary"]["status"] = "stale".into(),
        "template summary is stale without a reason",
    );
}

#[test]
fn step_absolute_artifact_rejected() {
    assert_state_rejected(
        |json| json["steps"]["audio"]["artifacts"] = serde_json::json!(["/elsewhere/audio.wav"]),
        "step audio has artifact /elsewhere/audio.wav outside the job folder",
    );
}

#[test]
fn step_climbing_artifact_rejected() {
    assert_state_rejected(
        |json| json["steps"]["audio"]["artifacts"] = serde_json::json!(["parts/../../x.wav"]),
        "step audio has artifact parts/../../x.wav outside the job folder",
    );
}

#[test]
fn template_absolute_artifact_rejected() {
    assert_state_rejected(
        |json| json["templates"]["summary"]["artifacts"] = serde_json::json!(["/tmp/summary.md"]),
        "template summary has artifact /tmp/summary.md outside the job folder",
    );
}

#[test]
fn stale_step_keeping_old_fingerprint_loads() {
    let (root, mut job) = done_job();
    job.mark_stale(&stale_from(StepName::Audio, Reason::Forced))
        .unwrap();
    let out = root.path().join("minuta-out");
    let source = root.path().join("clase-4.mp4");
    let reopened = Job::open(&out, "clase-4", &[source], false).unwrap();
    assert_eq!(reopened.state, job.state);
}

#[test]
fn unknown_job_state_field_rejected() {
    let (err, file) = reopen_after_edit(|json| {
        json["extra"] = 1.into();
    });
    assert_eq!(
        err,
        format!(
            "job error: invalid {}: unknown field `extra`, \
             expected one of `version`, `name`, `sources`, `steps`, `templates`",
            file.display()
        )
    );
}

#[test]
fn stale_name_refreshed_on_reopen() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    fs::rename(&job.dir, out.join("clase-5")).unwrap();

    let job = Job::open(&out, "clase-5", &[source], false).unwrap();
    assert_eq!(job.state.name, "clase-5");
    assert_eq!(read_json(&layout::state_file(&job.dir))["name"], "clase-5");
}

#[test]
fn layout_paths_are_under_minuta_and_assets() {
    let dir = Path::new("/jobs/talk");
    assert_eq!(layout::ASSETS_DIR, "assets");
    assert_eq!(layout::STATE_DIR, ".minuta");
    let expected = [
        (layout::state_file(dir), "/jobs/talk/.minuta/job.json"),
        (layout::probe_file(dir), "/jobs/talk/.minuta/probe.json"),
        (layout::call_cache_dir(dir), "/jobs/talk/.minuta/llm-cache"),
        (
            layout::implicit_md(dir, "youtube"),
            "/jobs/talk/assets/youtube.md",
        ),
        (
            layout::raw_whisper(dir),
            "/jobs/talk/.minuta/whisper-raw.json",
        ),
        (
            layout::transcript_json(dir),
            "/jobs/talk/assets/transcript.json",
        ),
        (layout::audio_file(dir), "/jobs/talk/assets/audio.wav"),
        (
            layout::joined_file(dir, "mp4"),
            "/jobs/talk/assets/joined.mp4",
        ),
        (
            layout::joined_file(dir, "mkv"),
            "/jobs/talk/assets/joined.mkv",
        ),
        (
            layout::template_json(dir, "youtube"),
            "/jobs/talk/assets/youtube.json",
        ),
    ];
    for (path, want) in expected {
        assert_eq!(path, Path::new(want));
    }
}

#[test]
fn write_atomic_creates_missing_parent() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("a").join("b").join("file.txt");
    write_atomic(&path, |tmp| Ok(fs::write(tmp, b"x")?)).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"x");
    assert!(!path.with_extension("txt.tmp").exists());
}

#[test]
fn open_cleans_tmp_in_all_tiers() {
    let (_root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let assets = job.dir.join(layout::ASSETS_DIR);
    fs::create_dir_all(&assets).unwrap();
    let tmps = [
        job.dir.join("transcript.txt.tmp"),
        assets.join("audio.wav.tmp"),
        layout::state_file(&job.dir).with_extension("json.tmp"),
    ];
    let keep = assets.join("audio.wav");
    fs::write(&keep, b"good").unwrap();
    for tmp in &tmps {
        fs::write(tmp, b"partial").unwrap();
    }

    Job::open(&out, "clase-4", &[source], false).unwrap();
    for tmp in &tmps {
        assert!(!tmp.exists(), "{} should be removed", tmp.display());
    }
    assert!(keep.exists());
}

#[test]
fn flat_layout_job_is_treated_as_new() {
    let (_root, out, source) = setup();
    let dir = out.join("clase-4");
    fs::create_dir_all(&dir).unwrap();
    // A job made by an earlier version, for other sources: it must not conflict.
    let old = br#"{"version":1,"name":"clase-4","sources":[{"path":"/elsewhere.mp4","size":1,"mtime":1}],"steps":{}}"#;
    fs::write(dir.join("job.json"), old).unwrap();
    fs::write(dir.join("transcript.json"), b"old").unwrap();

    let job = Job::open(&out, "clase-4", &[source], false).unwrap();
    assert!(layout::state_file(&job.dir).exists());
    assert_eq!(fs::read(dir.join("job.json")).unwrap(), old);
    assert_eq!(fs::read(dir.join("transcript.json")).unwrap(), b"old");
    assert!(job.state.steps.is_empty());
}

#[test]
fn call_cache_lives_in_minuta() {
    let root = tempfile::tempdir().unwrap();
    let cache = minuta::llm::call::CallCache::new(root.path());
    assert_eq!(
        cache.dir(),
        root.path().join(".minuta").join("llm-cache").as_path()
    );
}

/// A job whose `tier` folder (`.minuta` or `assets`) is a symlink to a folder outside it
/// holding a `victim.tmp` file. Returns the job, the outside folder and the victim.
fn job_with_symlinked_tier(tier: &str) -> (TempDir, Job, PathBuf, PathBuf) {
    let (root, out, source) = setup();
    let job = Job::open(&out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let victim = outside.join("victim.tmp");
    fs::write(&victim, b"keep").unwrap();
    let link = job.dir.join(tier);
    if link.is_dir() {
        fs::remove_dir_all(&link).unwrap();
    }
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    (root, job, outside, victim)
}

#[test]
fn clean_tmp_refuses_a_symlinked_tier() {
    for tier in [layout::STATE_DIR, layout::ASSETS_DIR] {
        let (_root, job, _outside, victim) = job_with_symlinked_tier(tier);
        let err = job.clean_tmp().unwrap_err();
        assert!(matches!(err, Error::Job(_)), "{tier}: {err}");
        assert!(err.to_string().contains(tier), "{err}");
        assert_eq!(fs::read(&victim).unwrap(), b"keep", "{tier}");
    }
}

#[test]
fn write_atomic_refuses_a_symlinked_parent() {
    let (_root, job, outside, _victim) = job_with_symlinked_tier(layout::ASSETS_DIR);
    let path = layout::audio_file(&job.dir);
    let err = write_atomic_in(&job.dir, &path, |tmp| Ok(fs::write(tmp, b"x")?)).unwrap_err();
    assert!(matches!(err, Error::Job(_)), "{err}");
    assert!(!outside.join("audio.wav").exists());
}

#[test]
fn write_atomic_does_not_create_through_a_symlinked_ancestor_tier() {
    let (_root, job, outside, _victim) = job_with_symlinked_tier(layout::STATE_DIR);
    let path = layout::call_cache_dir(&job.dir).join("k.txt");
    let err = write_atomic_in(&job.dir, &path, |tmp| Ok(fs::write(tmp, b"x")?)).unwrap_err();
    assert!(matches!(err, Error::Job(_)), "{err}");
    assert!(!outside.join("llm-cache").exists());
}

#[test]
fn job_folders_named_like_tiers_or_inside_symlinks_work() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    fs::create_dir(&real).unwrap();
    let source = root.path().join("clase-4.mp4");
    fs::write(&source, b"source bytes").unwrap();
    // `out` itself is a symlink named `assets`, and the job folder is a symlink named `.minuta`.
    let linked_out = root.path().join(layout::ASSETS_DIR);
    std::os::unix::fs::symlink(&real, &linked_out).unwrap();
    let job = Job::open(&linked_out, "clase-4", std::slice::from_ref(&source), false).unwrap();
    write_atomic_in(&job.dir, &layout::audio_file(&job.dir), |tmp| {
        Ok(fs::write(tmp, b"x")?)
    })
    .unwrap();
    job.clean_tmp().unwrap();

    let real_job = root.path().join("real-job");
    fs::create_dir(&real_job).unwrap();
    let hidden = root.path().join(layout::STATE_DIR);
    std::os::unix::fs::symlink(&real_job, &hidden).unwrap();
    let job = Job::open(root.path(), layout::STATE_DIR, &[source], false).unwrap();
    assert_eq!(job.dir, fs::canonicalize(&real_job).unwrap());
    write_atomic_in(&job.dir, &layout::transcript_json(&job.dir), |tmp| {
        Ok(fs::write(tmp, b"x")?)
    })
    .unwrap();
    job.save().unwrap();
    job.clean_tmp().unwrap();
}
