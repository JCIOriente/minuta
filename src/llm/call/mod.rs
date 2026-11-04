//! Call cache, retries and cleanup.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use super::Backend;
use crate::error::{Error, Result};
use crate::fields::Violation;

mod cache;
mod clean;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;

pub use cache::CallCache;
use cache::{cache_key, json_cache_key};
pub use clean::clean;

/// Longest single sleep, so `interrupt` is noticed promptly.
const SLEEP_SLICE: Duration = Duration::from_millis(25);

/// Retry policy: `retries` is the total number of attempts, `backoff` the first delay.
pub struct Policy {
    pub retries: u32,
    pub backoff: Duration,
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

/// Outcome of a `call_json` answer checked by `accept`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer<T> {
    Accepted(T),
    /// `text` is the cleaned answer, for a repair call.
    Rejected {
        text: String,
        violations: Vec<Violation>,
    },
}

/// Runs one `complete_json` call through the cache. The cache key adds the schema (compact
/// JSON). `accept` gets the cleaned, non-empty text; only an accepted answer is written to
/// the cache, and a cache hit that `accept` rejects counts as a miss (ADR-0006). `refresh`
/// skips the cache read. Transient failures, including empty output, are retried per `policy`.
// Signature fixed by the plan's shared interfaces.
#[allow(clippy::too_many_arguments)]
pub fn call_json<T>(
    backend: &dyn Backend,
    cache: &CallCache,
    policy: &Policy,
    system: &str,
    user: &str,
    schema: &serde_json::Value,
    refresh: bool,
    interrupt: &AtomicBool,
    accept: &dyn Fn(&str) -> std::result::Result<T, Vec<Violation>>,
) -> Result<Answer<T>> {
    let key = json_cache_key(backend, system, user, schema);
    cached_call(
        backend,
        cache,
        policy,
        &key,
        refresh,
        interrupt,
        &sleep_interruptibly,
        &|| backend.complete_json(system, user, schema, interrupt),
        &|text| match accept(text) {
            Ok(value) => Decision::Commit(Answer::Accepted(value)),
            Err(violations) => Decision::Skip(Answer::Rejected {
                text: text.to_string(),
                violations,
            }),
        },
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
    cached_call(
        backend,
        cache,
        policy,
        &key,
        refresh,
        interrupt,
        sleep,
        &|| backend.complete(system, user, interrupt),
        &|text| Decision::Commit(text.to_string()),
    )
}

/// What to do with a cleaned answer: `Commit` writes it to the cache (and a cache hit is
/// used), `Skip` returns it without writing (and a cache hit is treated as a miss).
enum Decision<R> {
    Commit(R),
    Skip(R),
}

/// The cache read, retry loop and cache write shared by `call` and `call_json`. `invoke`
/// makes one backend request; `decide` judges each cleaned answer, cached or fresh.
// Each argument is an independent input of the call; bundling them would only move the list.
#[allow(clippy::too_many_arguments)]
fn cached_call<R>(
    backend: &dyn Backend,
    cache: &CallCache,
    policy: &Policy,
    key: &str,
    refresh: bool,
    interrupt: &AtomicBool,
    sleep: Sleeper,
    invoke: &dyn Fn() -> Result<String>,
    decide: &dyn Fn(&str) -> Decision<R>,
) -> Result<R> {
    if !refresh {
        if let Some(text) = cache.read(key)? {
            if let Decision::Commit(value) = decide(&text) {
                return Ok(value);
            }
        }
    }
    let attempts = policy.retries.max(1);
    let mut attempt = 1;
    loop {
        if interrupt.load(Ordering::SeqCst) {
            return Err(Error::Interrupted);
        }
        let result = invoke().and_then(|raw| non_empty(backend, clean(&raw)));
        match result {
            Ok(text) => {
                return match decide(&text) {
                    Decision::Commit(value) => {
                        cache.write(key, &text)?;
                        Ok(value)
                    }
                    Decision::Skip(value) => Ok(value),
                };
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
