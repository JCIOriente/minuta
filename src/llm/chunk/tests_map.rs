//! Tests for the single call and the map calls: fit, chunk order, concurrency, errors, events.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::thread;
use std::time::Duration;

use super::test_support::{
    boom, chunk_total, fits, is_final, map_calls, numbered_lines, request, Recorder, BODY, LANG,
};
use super::*;

const DEADLINE: Duration = Duration::from_secs(10);

/// A counter threads can raise and wait on, failing instead of hanging after DEADLINE.
struct Gate {
    count: Mutex<usize>,
    changed: Condvar,
}

impl Gate {
    fn new() -> Self {
        Gate {
            count: Mutex::new(0),
            changed: Condvar::new(),
        }
    }
    fn raise(&self) {
        *self.count.lock().unwrap() += 1;
        self.changed.notify_all();
    }
    /// Block until the count reaches `n`; panic with `failure` after DEADLINE.
    fn wait_for(&self, n: usize, failure: &str) {
        let guard = self.count.lock().unwrap();
        let (_count, timeout) = self
            .changed
            .wait_timeout_while(guard, DEADLINE, |count| *count < n)
            .unwrap();
        assert!(!timeout.timed_out(), "{failure}");
    }
    /// Block until the count exceeds `n` or `timeout` passes, whichever comes first.
    fn wait_above(&self, n: usize, timeout: Duration) {
        let guard = self.count.lock().unwrap();
        let (_count, _timeout) = self
            .changed
            .wait_timeout_while(guard, timeout, |count| *count <= n)
            .unwrap();
    }
}

#[test]
fn fits_makes_one_call() {
    let input = "[00:00:00] hello there";
    let rec = Recorder::new();
    let complete = |s: &str, u: &str| {
        rec.record(s, u);
        Ok("the minutes".to_string())
    };
    let mut events = Vec::new();
    let out = generate(&request(input, 1000, 4), &complete, &mut |e| events.push(e)).unwrap();
    assert_eq!(
        out,
        Generated {
            text: "the minutes".into(),
            calls: 1
        }
    );
    let calls = rec.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].0,
        format!("{BODY}\n\n{}", language_instruction(LANG))
    );
    assert_eq!(calls[0].1, input);
    assert!(events.is_empty(), "{events:?}");
}

#[test]
fn system_alone_too_large_fails() {
    let body = "word ".repeat(400);
    let req = Request {
        body: &body,
        output_lang: LANG,
        input: "hello",
        context_tokens: 200,
        concurrency: 1,
    };
    let err = generate(&req, &|_: &str, _: &str| Ok("x".into()), &mut |_| {}).unwrap_err();
    assert!(
        matches!(err, Error::Template { ref name, .. } if name.is_empty()),
        "{err}"
    );
}

#[test]
fn chunk_order_kept() {
    let input = numbered_lines(200);
    let rec = Recorder::new();
    let complete = |s: &str, u: &str| {
        rec.record(s, u);
        Ok("n".to_string())
    };
    let mut events = Vec::new();
    generate(&request(&input, 300, 3), &complete, &mut |e| events.push(e)).unwrap();
    let total = chunk_total(&events);
    assert!(total > 2, "expected several chunks, got {total}");
    let maps = map_calls(&rec.calls(), total);
    for (i, (s, _)) in maps.iter().enumerate() {
        assert!(
            s.contains(&format!("This is part {} of {total}.", i + 1)),
            "{s}"
        );
    }
    let joined: Vec<&str> = maps.iter().map(|(_, u)| u.as_str()).collect();
    assert_eq!(joined.join("\n"), input);
}

#[test]
fn concurrency_limit_respected() {
    const CONCURRENCY: usize = 3;
    let input = numbered_lines(200);
    let started = AtomicUsize::new(0);
    let current = AtomicUsize::new(0);
    let peak = AtomicUsize::new(0);
    // The first CONCURRENCY calls wait for each other, so they overlap for certain. They
    // then hold on for up to 100 ms more, giving a worker beyond the limit time to start
    // a call and raise the peak; for correct code that wait simply times out.
    let arrived = Gate::new();
    let complete = |s: &str, _: &str| {
        if !is_final(s) {
            let now = current.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            arrived.raise();
            if started.fetch_add(1, Ordering::SeqCst) < CONCURRENCY {
                arrived.wait_for(CONCURRENCY, "fewer than 3 calls overlapped");
                arrived.wait_above(CONCURRENCY, Duration::from_millis(100));
            }
            current.fetch_sub(1, Ordering::SeqCst);
        }
        Ok("n".to_string())
    };
    let mut events = Vec::new();
    generate(&request(&input, 300, CONCURRENCY), &complete, &mut |e| {
        events.push(e)
    })
    .unwrap();
    assert!(chunk_total(&events) > CONCURRENCY);
    assert_eq!(peak.load(Ordering::SeqCst), CONCURRENCY);
}

#[test]
fn fit_boundary_is_inclusive() {
    let context = 1000;
    let system = format!("{BODY}\n\n{}", language_instruction(LANG));
    let room = context - context / 4 - estimate_tokens(&system);
    let exact = "x".repeat(room * 3);
    assert!(fits(&system, &exact, context));
    let cases = [(exact.clone(), true), (format!("{exact}x"), false)];
    for (input, one_call) in cases {
        let count = AtomicUsize::new(0);
        let complete = |_: &str, _: &str| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok("n".to_string())
        };
        let out = generate(&request(&input, context, 1), &complete, &mut |_| {}).unwrap();
        assert_eq!(out.calls == 1, one_call, "input of {} chars", input.len());
    }
}

#[test]
fn first_error_stops_scheduling() {
    let input = numbered_lines(200);
    let count = AtomicUsize::new(0);
    let complete = |_: &str, _: &str| {
        if count.fetch_add(1, Ordering::SeqCst) == 1 {
            Err(boom())
        } else {
            Ok("n".to_string())
        }
    };
    let err = generate(&request(&input, 300, 1), &complete, &mut |_| {}).unwrap_err();
    assert!(matches!(err, Error::Llm { ref message, .. } if message == "boom"));
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[test]
fn first_error_waits_for_in_flight_calls() {
    let input = numbered_lines(200);
    let started = AtomicUsize::new(0);
    let finished = AtomicUsize::new(0);
    // Chunk 0 fails only once chunk 1 is in flight; chunk 1 ends only after chunk 0 failed,
    // so generate must wait for it. No timing decides the outcome.
    let second_started = Gate::new();
    let failed = Gate::new();
    let complete = |_: &str, u: &str| {
        started.fetch_add(1, Ordering::SeqCst);
        let result = if u.starts_with("line 000") {
            second_started.wait_for(1, "the second call never started");
            failed.raise();
            Err(boom())
        } else {
            second_started.raise();
            failed.wait_for(1, "the first call never failed");
            Ok("n".to_string())
        };
        finished.fetch_add(1, Ordering::SeqCst);
        result
    };
    let err = generate(&request(&input, 300, 2), &complete, &mut |_| {}).unwrap_err();
    assert!(matches!(err, Error::Llm { .. }), "{err}");
    let started = started.load(Ordering::SeqCst);
    assert!(started >= 2, "only {started} calls started");
    assert_eq!(finished.load(Ordering::SeqCst), started);
}

#[test]
fn events_in_order() {
    let input = numbered_lines(200);
    let caller = thread::current().id();
    let complete = |_: &str, _: &str| Ok("n".to_string());
    let mut events = Vec::new();
    generate(&request(&input, 300, 3), &complete, &mut |e| {
        assert_eq!(thread::current().id(), caller);
        events.push(e);
    })
    .unwrap();
    let total = chunk_total(&events);
    let mut want: Vec<Event> = (1..=total)
        .map(|done| Event::Chunk { done, total })
        .collect();
    want.push(Event::Consolidating);
    assert_eq!(events, want);
}
