use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::job::{
    fingerprint, plan_stale, source_record, step_fingerprint, Ctx, Job, Reason, Source, Step,
    StepName,
};

use super::setup;

#[test]
fn fingerprint_is_stable_and_separates_parts() {
    let a = fingerprint(&[b"ab", b"c"]);
    assert_eq!(a, fingerprint(&[b"ab", b"c"]));
    assert_eq!(a.len(), 64);
    assert_ne!(a, fingerprint(&[b"a", b"bc"]));
    assert_ne!(a, fingerprint(&[b"ab", b"d"]));
}

#[test]
fn fingerprint_matches_known_vector() {
    assert_eq!(
        fingerprint(&[b"ab", b"c"]),
        "43ee655579de01ca739b3f95c1c2d3f46d353b2c0df818064ea594506cdb2617"
    );
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

    fn run(&self, _ctx: &Ctx) -> crate::error::Result<()> {
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

/// Longer than one read, so every chunk has to reach the hash.
#[test]
fn source_hash_matches_known_vector() {
    let (_root, _out, source) = setup();
    fs::write(&source, vec![b'a'; 200 * 1024]).unwrap();
    assert_eq!(
        source_record(&source, None).unwrap().hash,
        "4b4f0f46ac02d177dea0ab36a66a657840e2fb98b20bb27a688db4d8ea9cd22c"
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
