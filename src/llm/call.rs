//! Call cache, retries and cleanup.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use super::Backend;
use crate::error::{Error, Result};
use crate::job::{fingerprint, write_atomic};

/// Longest single sleep, so `interrupt` is noticed promptly.
const SLEEP_SLICE: Duration = Duration::from_millis(25);

/// Retry policy: `retries` is the total number of attempts, `backoff` the first delay.
pub struct Policy {
    pub retries: u32,
    pub backoff: Duration,
}

/// Per-job cache of cleaned LLM outputs in `<job>/llm-cache`.
pub struct CallCache {
    dir: PathBuf,
}

impl CallCache {
    pub fn new(job_dir: &Path) -> CallCache {
        CallCache {
            dir: job_dir.join("llm-cache"),
        }
    }

    fn entry(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.txt"))
    }

    fn read(&self, key: &str) -> Result<Option<String>> {
        let path = self.entry(key);
        match fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Io(io::Error::new(
                e.kind(),
                format!("{}: {e}", path.display()),
            ))),
        }
    }

    fn write(&self, key: &str, text: &str) -> Result<()> {
        fs::create_dir_all(&self.dir)?;
        write_atomic(&self.entry(key), |tmp| {
            let mut f = fs::File::create(tmp)?;
            f.write_all(text.as_bytes())?;
            Ok(())
        })
    }
}

fn cache_key(backend: &dyn Backend, system: &str, user: &str) -> String {
    let context = backend.context_tokens().to_string();
    fingerprint(&[
        backend.id().as_bytes(),
        context.as_bytes(),
        system.as_bytes(),
        user.as_bytes(),
    ])
}

/// Runs one LLM call through the cache. `refresh` skips the cache read and overwrites the
/// entry. Transient failures, including empty output, are retried per `policy`.
pub fn call(
    backend: &dyn Backend,
    cache: &CallCache,
    policy: &Policy,
    system: &str,
    user: &str,
    refresh: bool,
    interrupt: &AtomicBool,
) -> Result<String> {
    let key = cache_key(backend, system, user);
    if !refresh {
        if let Some(text) = cache.read(&key)? {
            return Ok(text);
        }
    }
    let attempts = policy.retries.max(1);
    let mut attempt = 1;
    loop {
        if interrupt.load(Ordering::SeqCst) {
            return Err(Error::Interrupted);
        }
        let result = backend
            .complete(system, user, interrupt)
            .and_then(|raw| non_empty(backend, clean(&raw)));
        match result {
            Ok(text) => {
                cache.write(&key, &text)?;
                return Ok(text);
            }
            Err(Error::Llm {
                transient: true, ..
            }) if attempt < attempts => {
                sleep_interruptibly(delay(policy.backoff, attempt), interrupt)?;
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

fn non_empty(backend: &dyn Backend, text: String) -> Result<String> {
    if text.is_empty() {
        return Err(Error::Llm {
            backend: backend.id().to_string(),
            message: "empty response".into(),
            transient: true,
        });
    }
    Ok(text)
}

/// `backoff * 2^(attempt-1)`, saturating.
fn delay(backoff: Duration, attempt: u32) -> Duration {
    let factor = 1u32.checked_shl(attempt - 1).unwrap_or(u32::MAX);
    backoff.saturating_mul(factor)
}

fn sleep_interruptibly(total: Duration, interrupt: &AtomicBool) -> Result<()> {
    let deadline = Instant::now() + total;
    loop {
        if interrupt.load(Ordering::SeqCst) {
            return Err(Error::Interrupted);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(());
        }
        thread::sleep(left.min(SLEEP_SLICE));
    }
}

/// Trims, drops a leading `<think>…</think>` block and unwraps output that is exactly one
/// fenced code block. Output with prose around a fence, or with several fences, is kept.
pub fn clean(output: &str) -> String {
    let mut text = output.trim();
    if let Some(rest) = text.strip_prefix("<think>") {
        if let Some((_, after)) = rest.split_once("</think>") {
            text = after.trim();
        }
    }
    unwrap_fence(text).unwrap_or(text).to_string()
}

fn unwrap_fence(text: &str) -> Option<&str> {
    let (first, rest) = text.split_once('\n')?;
    if !first.starts_with("```") {
        return None;
    }
    let body = rest.strip_suffix("```")?;
    let body = body.strip_suffix('\n').unwrap_or(body);
    if body.lines().any(|l| l.trim_start().starts_with("```")) {
        return None;
    }
    Some(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::fake::ScriptedBackend;

    fn transient() -> Error {
        Error::Llm {
            backend: "fake:x".into(),
            message: "flaky".into(),
            transient: true,
        }
    }

    fn policy(retries: u32) -> Policy {
        Policy {
            retries,
            backoff: Duration::ZERO,
        }
    }

    fn backend(replies: Vec<Result<String>>) -> ScriptedBackend {
        ScriptedBackend::new("fake:x", 1000, replies)
    }

    fn run(b: &ScriptedBackend, cache: &CallCache, p: &Policy, refresh: bool) -> Result<String> {
        call(b, cache, p, "sys", "usr", refresh, &AtomicBool::new(false))
    }

    #[test]
    fn cache_key_sensitive_to_each_part() {
        let base = ScriptedBackend::new("fake:x", 1000, vec![]);
        let k = cache_key(&base, "sys", "usr");
        let other_id = ScriptedBackend::new("fake:y", 1000, vec![]);
        let other_ctx = ScriptedBackend::new("fake:x", 2000, vec![]);
        assert_ne!(k, cache_key(&other_id, "sys", "usr"), "backend id");
        assert_ne!(k, cache_key(&other_ctx, "sys", "usr"), "context tokens");
        assert_ne!(k, cache_key(&base, "sys2", "usr"), "system");
        assert_ne!(k, cache_key(&base, "sys", "usr2"), "user");
        assert_eq!(k, cache_key(&base, "sys", "usr"), "stable");
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
        let path = dir.path().join("llm-cache").join(format!("{key}.txt"));
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
    fn cache_read_error_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CallCache::new(dir.path());
        let b = backend(vec![Ok("x".into())]);
        let key = cache_key(&b, "sys", "usr");
        // A directory in place of the entry: reading fails with something other than NotFound.
        fs::create_dir_all(dir.path().join("llm-cache").join(format!("{key}.txt"))).unwrap();
        let err = run(&b, &cache, &policy(1), false).unwrap_err();
        assert!(matches!(err, Error::Io(_)), "{err}");
        assert!(b.calls().is_empty());
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
        let started = Instant::now();
        let result = thread::scope(|s| {
            s.spawn(|| {
                thread::sleep(Duration::from_millis(50));
                stop.store(true, Ordering::SeqCst);
            });
            call(&b, &cache, &p, "sys", "usr", false, &stop)
        });
        assert!(matches!(result, Err(Error::Interrupted)));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(b.calls().len(), 1);
    }

    #[test]
    fn clean_strips_think() {
        assert_eq!(clean("<think>hmm\nmore</think>\n\n  answer \n"), "answer");
    }

    #[test]
    fn clean_unwraps_fence() {
        assert_eq!(clean("```markdown\n# T\n\ntext\n```"), "# T\n\ntext");
        assert_eq!(clean("  ```\nplain\n```\n"), "plain");
    }

    #[test]
    fn clean_keeps_inner_fences() {
        let prose = "Intro\n```rust\nfn a() {}\n```\nOutro";
        assert_eq!(clean(prose), prose);
        let two = "```a\nx\n```\ntext\n```b\ny\n```";
        assert_eq!(clean(two), two);
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
        assert!(!dir.path().join("other").join("llm-cache").exists());
    }
}
