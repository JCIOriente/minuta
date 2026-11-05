//! Tests for `call` and `call_json`: cache use, retries, backoff, cleanup and JSON answers.

use std::fs;
use std::path::{Path, PathBuf};

use super::test_support::{backend, policy, run, schema};
use super::*;
use crate::job::layout;
use crate::llm::fake::ScriptedBackend;

fn transient() -> Error {
    Error::Llm {
        backend: "fake:x".into(),
        message: "flaky".into(),
        transient: true,
    }
}

#[test]
fn cache_hit_skips_backend() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CallCache::new(dir.path());
    let b = backend(vec![Ok("first".into()), Ok("second".into())]);
    assert_eq!(run(&b, &cache, &policy(1), false).unwrap(), "first");
    assert_eq!(run(&b, &cache, &policy(1), false).unwrap(), "first");
    assert_eq!(b.calls().len(), 1);
    let key = cache_key(&b, "sys", "usr");
    let path = layout::call_cache_dir(dir.path()).join(format!("{key}.txt"));
    assert_eq!(fs::read_to_string(path).unwrap(), "first");
}

#[test]
fn refresh_skips_read_and_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CallCache::new(dir.path());
    let b = backend(vec![Ok("first".into()), Ok("second".into())]);
    run(&b, &cache, &policy(1), false).unwrap();
    assert_eq!(run(&b, &cache, &policy(1), true).unwrap(), "second");
    assert_eq!(run(&b, &cache, &policy(1), false).unwrap(), "second");
    assert_eq!(b.calls().len(), 2);
}

#[test]
fn transient_retried_until_success() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CallCache::new(dir.path());
    let b = backend(vec![Err(transient()), Err(transient()), Ok("ok".into())]);
    assert_eq!(run(&b, &cache, &policy(3), false).unwrap(), "ok");
    assert_eq!(b.calls().len(), 3);
}

#[test]
fn retries_exhausted_returns_last_error() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CallCache::new(dir.path());
    let last = Error::Llm {
        backend: "fake:x".into(),
        message: "last".into(),
        transient: true,
    };
    let b = backend(vec![Err(transient()), Err(last), Ok("unused".into())]);
    let err = run(&b, &cache, &policy(2), false).unwrap_err();
    assert!(
        matches!(err, Error::Llm { ref message, .. } if message == "last"),
        "{err}"
    );
    assert_eq!(b.calls().len(), 2);
}

#[test]
fn non_transient_not_retried() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CallCache::new(dir.path());
    let fatal = Error::Llm {
        backend: "fake:x".into(),
        message: "fatal".into(),
        transient: false,
    };
    let b = backend(vec![Err(fatal), Ok("unused".into())]);
    assert!(run(&b, &cache, &policy(3), false).is_err());
    assert_eq!(b.calls().len(), 1);
    let b = backend(vec![Err(Error::Interrupted), Ok("unused".into())]);
    assert!(matches!(
        run(&b, &cache, &policy(3), false),
        Err(Error::Interrupted)
    ));
    assert_eq!(b.calls().len(), 1);
}

#[test]
fn call_sleeps_backoff_then_double() {
    use std::cell::RefCell;
    let dir = tempfile::tempdir().unwrap();
    let cache = CallCache::new(dir.path());
    let b = backend(vec![Err(transient()), Err(transient()), Ok("ok".into())]);
    let p = Policy {
        retries: 3,
        backoff: Duration::from_millis(7),
    };
    let delays = RefCell::new(Vec::new());
    let sleep = |d: Duration, _: &AtomicBool| {
        delays.borrow_mut().push(d);
        Ok(())
    };
    let stop = AtomicBool::new(false);
    let out = call_with(&b, &cache, &p, "sys", "usr", false, &stop, &sleep).unwrap();
    assert_eq!(out, "ok");
    assert_eq!(
        *delays.borrow(),
        vec![Duration::from_millis(7), Duration::from_millis(14)]
    );
}

#[test]
fn cleaned_text_is_returned_and_cached() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CallCache::new(dir.path());
    let b = backend(vec![Ok("<think>x</think>\n```md\nbody\n```".into())]);
    assert_eq!(run(&b, &cache, &policy(1), false).unwrap(), "body");
    let key = cache_key(&b, "sys", "usr");
    let path = layout::call_cache_dir(dir.path()).join(format!("{key}.txt"));
    assert_eq!(fs::read_to_string(path).unwrap(), "body");
}

#[test]
fn backoff_doubles() {
    let d = Duration::from_millis(10);
    assert_eq!(delay(d, 1), d);
    assert_eq!(delay(d, 2), d * 2);
    assert_eq!(delay(d, 3), d * 4);
    assert_eq!(delay(d, 40), d.saturating_mul(u32::MAX));
}

#[test]
fn backoff_interrupt_returns_quickly() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CallCache::new(dir.path());
    let b = backend(vec![Err(transient()), Ok("unused".into())]);
    let p = Policy {
        retries: 3,
        backoff: Duration::from_secs(30),
    };
    let stop = AtomicBool::new(false);
    let (result, set_at) = thread::scope(|s| {
        let setter = s.spawn(|| {
            thread::sleep(Duration::from_millis(50));
            stop.store(true, Ordering::SeqCst);
            Instant::now()
        });
        let result = call(&b, &cache, &p, "sys", "usr", false, &stop);
        let returned = Instant::now();
        (result, (setter.join().unwrap(), returned))
    });
    assert!(matches!(result, Err(Error::Interrupted)));
    let (set, returned) = set_at;
    // The sleep slice is 25 ms; the extra margin absorbs CI scheduler
    // jitter, and a slice regression to >= 250 ms still fails the test.
    assert!(returned.duration_since(set) < Duration::from_millis(250));
    assert_eq!(b.calls().len(), 1);
}

#[test]
fn empty_after_clean_is_transient() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CallCache::new(dir.path());
    let b = backend(vec![Ok("<think>x</think>  ".into()), Ok("real".into())]);
    assert_eq!(run(&b, &cache, &policy(2), false).unwrap(), "real");
    let b = backend(vec![Ok("  ".into())]);
    let cache2 = CallCache::new(&dir.path().join("other"));
    let err = run(&b, &cache2, &policy(1), false).unwrap_err();
    assert!(
        matches!(
            err,
            Error::Llm {
                transient: true,
                ..
            }
        ),
        "{err}"
    );
    assert!(!dir
        .path()
        .join("other")
        .join(".minuta")
        .join("llm-cache")
        .exists());
}

/// Accepts text starting with `good`; anything else gets one violation naming it.
fn accept_good(text: &str) -> std::result::Result<String, Vec<Violation>> {
    if text.starts_with("good") {
        Ok(format!("parsed {text}"))
    } else {
        Err(vec![Violation(format!("not good: {text}"))])
    }
}

fn run_json(
    b: &ScriptedBackend,
    cache: &CallCache,
    p: &Policy,
    refresh: bool,
) -> Result<Answer<String>> {
    let stop = AtomicBool::new(false);
    let s = schema();
    call_json(b, cache, p, "sys", "usr", &s, refresh, &stop, &accept_good)
}

fn json_entry(dir: &Path, b: &ScriptedBackend) -> PathBuf {
    let key = json_cache_key(b, "sys", "usr", &schema());
    layout::call_cache_dir(dir).join(format!("{key}.txt"))
}

#[test]
fn accepted_answer_cached() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CallCache::new(dir.path());
    let b = backend(vec![Ok("```json\ngood\n```".into()), Ok("unused".into())]);
    let first = run_json(&b, &cache, &policy(1), false).unwrap();
    assert_eq!(first, Answer::Accepted("parsed good".to_string()));
    assert_eq!(
        b.schemas(),
        vec![Some(schema())],
        "complete_json with schema"
    );
    assert_eq!(
        fs::read_to_string(json_entry(dir.path(), &b)).unwrap(),
        "good"
    );
    let second = run_json(&b, &cache, &policy(1), false).unwrap();
    assert_eq!(second, Answer::Accepted("parsed good".to_string()));
    assert_eq!(b.calls().len(), 1, "second call served from the cache");
}

#[test]
fn rejected_answer_not_cached() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CallCache::new(dir.path());
    let b = backend(vec![Ok("<think>x</think>\n```json\nbad\n```".into())]);
    let answer = run_json(&b, &cache, &policy(1), false).unwrap();
    assert_eq!(
        answer,
        Answer::Rejected {
            text: "bad".into(),
            violations: vec![Violation("not good: bad".into())],
        }
    );
    assert!(!layout::call_cache_dir(dir.path()).try_exists().unwrap());
}

#[test]
fn rejected_cache_hit_is_miss() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CallCache::new(dir.path());
    let b = backend(vec![Ok("good".into())]);
    let entry = json_entry(dir.path(), &b);
    fs::create_dir_all(entry.parent().unwrap()).unwrap();
    fs::write(&entry, "damaged").unwrap();
    let answer = run_json(&b, &cache, &policy(1), false).unwrap();
    assert_eq!(answer, Answer::Accepted("parsed good".to_string()));
    assert_eq!(b.calls().len(), 1);
    assert_eq!(fs::read_to_string(&entry).unwrap(), "good");
}

#[test]
fn json_refresh_skips_read() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CallCache::new(dir.path());
    let b = backend(vec![Ok("good".into()), Ok("good again".into())]);
    run_json(&b, &cache, &policy(1), false).unwrap();
    let answer = run_json(&b, &cache, &policy(1), true).unwrap();
    assert_eq!(answer, Answer::Accepted("parsed good again".to_string()));
    assert_eq!(
        b.calls().len(),
        2,
        "refresh calls the backend despite a valid entry"
    );
    let entry = json_entry(dir.path(), &b);
    assert_eq!(fs::read_to_string(entry).unwrap(), "good again");
}

#[test]
fn json_transient_retried() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CallCache::new(dir.path());
    let b = backend(vec![
        Err(transient()),
        Ok("<think>x</think>  ".into()),
        Ok("good".into()),
    ]);
    let answer = run_json(&b, &cache, &policy(3), false).unwrap();
    assert_eq!(answer, Answer::Accepted("parsed good".to_string()));
    assert_eq!(b.calls().len(), 3);
    assert_eq!(b.schemas(), vec![Some(schema()); 3]);
}

#[test]
fn json_retries_exhausted_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CallCache::new(dir.path());
    let b = backend(vec![Err(transient()), Ok("  ".into()), Ok("good".into())]);
    let err = run_json(&b, &cache, &policy(2), false).unwrap_err();
    assert!(
        matches!(err, Error::Llm { transient: true, ref message, .. } if message == "empty response"),
        "{err}"
    );
    assert_eq!(b.calls().len(), 2);
    assert!(!layout::call_cache_dir(dir.path()).try_exists().unwrap());
}

#[test]
fn json_interrupted() {
    let dir = tempfile::tempdir().unwrap();
    let cache = CallCache::new(dir.path());
    let b = backend(vec![Ok("good".into())]);
    let stop = AtomicBool::new(true);
    let s = schema();
    let result = call_json(
        &b,
        &cache,
        &policy(3),
        "sys",
        "usr",
        &s,
        false,
        &stop,
        &accept_good,
    );
    assert!(matches!(result, Err(Error::Interrupted)));
    assert!(b.calls().is_empty());
}
