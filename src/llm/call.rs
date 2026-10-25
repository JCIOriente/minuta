//! Call cache, retries and cleanup.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use super::Backend;
use crate::error::{Error, Result};
use crate::job::fingerprint;

/// Longest single sleep, so `interrupt` is noticed promptly.
const SLEEP_SLICE: Duration = Duration::from_millis(25);

/// Makes temp names unique across threads of this process.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn with_path(path: &Path, e: io::Error) -> Error {
    Error::Io(io::Error::new(e.kind(), format!("{}: {e}", path.display())))
}

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

    /// An empty entry counts as a miss: only non-empty text is ever stored.
    fn read(&self, key: &str) -> Result<Option<String>> {
        let path = self.entry(key);
        match fs::read_to_string(&path) {
            Ok(text) if text.is_empty() => Ok(None),
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(with_path(&path, e)),
        }
    }

    /// Writes to a temp file unique to this call, syncs it and renames it into place, so
    /// concurrent writers of one key never share a temp file.
    fn write(&self, key: &str, text: &str) -> Result<()> {
        fs::create_dir_all(&self.dir).map_err(|e| with_path(&self.dir, e))?;
        let path = self.entry(key);
        let tmp = self.dir.join(format!(
            "{key}.{}-{}.tmp",
            std::process::id(),
            TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut f = fs::File::create(&tmp).map_err(|e| with_path(&tmp, e))?;
            f.write_all(text.as_bytes())
                .and_then(|()| f.sync_all())
                .map_err(|e| with_path(&tmp, e))?;
            fs::rename(&tmp, &path).map_err(|e| with_path(&path, e))
        })();
        if result.is_ok() {
            // Best effort: persist the rename. Some platforms cannot sync a directory.
            let _ = fs::File::open(&self.dir).and_then(|d| d.sync_all());
        }
        if result.is_err() {
            // Best effort: `clean_tmp` removes whatever survives.
            let _ = fs::remove_file(&tmp);
        }
        result
    }

    /// Removes leftover `*.tmp` files in `llm-cache`; call only when no calls are in flight
    /// (at the start of a run). A missing cache dir is fine.
    pub fn clean_tmp(&self) -> Result<()> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(with_path(&self.dir, e)),
        };
        for entry in entries {
            let path = entry.map_err(|e| with_path(&self.dir, e))?.path();
            if path.extension().is_some_and(|ext| ext == "tmp") {
                fs::remove_file(&path).map_err(|e| with_path(&path, e))?;
            }
        }
        Ok(())
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
    call_with(
        backend,
        cache,
        policy,
        system,
        user,
        refresh,
        interrupt,
        &sleep_interruptibly,
    )
}

type Sleeper<'a> = &'a dyn Fn(Duration, &AtomicBool) -> Result<()>;

#[allow(clippy::too_many_arguments)]
fn call_with(
    backend: &dyn Backend,
    cache: &CallCache,
    policy: &Policy,
    system: &str,
    user: &str,
    refresh: bool,
    interrupt: &AtomicBool,
    sleep: Sleeper,
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
                sleep(delay(policy.backoff, attempt), interrupt)?;
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

fn backtick_run(line: &str) -> usize {
    line.chars().take_while(|&c| c == '`').count()
}

/// A line made only of backticks (a bare fence run), as its length.
fn bare_run(line: &str) -> Option<usize> {
    let line = line.trim();
    (!line.is_empty() && line.chars().all(|c| c == '`')).then_some(line.len())
}

/// Unwraps one fenced block: an opening run of n >= 3 backticks (plus an optional info
/// string), a last line that is a bare run of at least n, and no earlier bare run of at
/// least n (that would close the block early, so the text is several blocks).
fn unwrap_fence(text: &str) -> Option<&str> {
    let (first, rest) = text.split_once('\n')?;
    let open = backtick_run(first);
    if open < 3 {
        return None;
    }
    let (body, last) = match rest.rsplit_once('\n') {
        Some((body, last)) => (body, last),
        None => ("", rest),
    };
    if bare_run(last)? < open {
        return None;
    }
    if body.lines().any(|l| bare_run(l).is_some_and(|n| n >= open)) {
        return None;
    }
    Some(body.trim())
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
    fn complete_cache_key_unchanged() {
        // Computed on main before `call_json` existed: existing job folders keep their cache.
        let b = ScriptedBackend::new("fake:x", 1000, vec![]);
        assert_eq!(cache_key(&b, "s", "u"), "67d3590845e650cb4c38fd859fea4e204e5ab8ec39b79402422d9057f2b31226");
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
        let path = dir.path().join("llm-cache").join(format!("{key}.txt"));
        assert_eq!(fs::read_to_string(path).unwrap(), "body");
    }

    #[test]
    fn empty_cache_entry_is_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CallCache::new(dir.path());
        let b = backend(vec![Ok("fresh".into())]);
        let key = cache_key(&b, "sys", "usr");
        let cache_dir = dir.path().join("llm-cache");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(cache_dir.join(format!("{key}.txt")), "").unwrap();
        assert_eq!(run(&b, &cache, &policy(1), false).unwrap(), "fresh");
        assert_eq!(b.calls().len(), 1);
    }

    #[test]
    fn write_error_names_path() {
        let dir = tempfile::tempdir().unwrap();
        // A file where the cache dir should be.
        fs::write(dir.path().join("llm-cache"), "x").unwrap();
        let cache = CallCache::new(dir.path());
        let err = cache.write("k", "text").unwrap_err();
        assert!(err.to_string().contains("llm-cache"), "{err}");
    }

    #[test]
    fn rename_error_names_entry_path() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CallCache::new(dir.path());
        let entry = dir.path().join("llm-cache").join("k.txt");
        fs::create_dir_all(entry.join("child")).unwrap();
        let err = cache.write("k", "text").unwrap_err();
        assert!(err.to_string().contains("k.txt"), "{err}");
        let tmp_left = fs::read_dir(dir.path().join("llm-cache"))
            .unwrap()
            .any(|e| e.unwrap().path().extension().is_some_and(|x| x == "tmp"));
        assert!(!tmp_left);
    }

    #[test]
    fn clean_tmp_removes_only_tmp_files() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CallCache::new(dir.path());
        cache.clean_tmp().unwrap(); // missing dir is fine
        cache.write("k", "text").unwrap();
        let cache_dir = dir.path().join("llm-cache");
        fs::write(cache_dir.join("k.1-2.tmp"), "partial").unwrap();
        cache.clean_tmp().unwrap();
        assert!(!cache_dir.join("k.1-2.tmp").exists());
        assert_eq!(fs::read_to_string(cache_dir.join("k.txt")).unwrap(), "text");
    }

    #[test]
    fn write_leaves_no_tmp_files() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CallCache::new(dir.path());
        cache.write("k", "a").unwrap();
        cache.write("k", "b").unwrap();
        let names: Vec<_> = fs::read_dir(dir.path().join("llm-cache"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("k.txt")]);
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
    fn clean_strips_think() {
        assert_eq!(clean("<think>hmm\nmore</think>\n\n  answer \n"), "answer");
    }

    #[test]
    fn clean_unwraps_fence() {
        assert_eq!(clean("```markdown\n# T\n\ntext\n```"), "# T\n\ntext");
        assert_eq!(clean("  ```\nplain\n```\n"), "plain");
        assert_eq!(clean("````\nplain\n````"), "plain");
        assert_eq!(
            clean("````markdown\n# N\n```sh\nls\n```\n````"),
            "# N\n```sh\nls\n```"
        );
    }

    #[test]
    fn clean_trims_unwrapped_fence_body() {
        assert_eq!(clean("```\n   \n```"), "");
        assert_eq!(clean("```\n\n x \n\n```"), "x");
    }

    #[test]
    fn clean_leaves_non_leading_or_unclosed_think() {
        assert_eq!(
            clean(" answer <think>x</think> "),
            "answer <think>x</think>"
        );
        assert_eq!(clean("<think>unclosed answer\n"), "<think>unclosed answer");
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
