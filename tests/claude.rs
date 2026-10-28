#![cfg(unix)]

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use minuta::error::Error;
use minuta::llm::call::{call, CallCache, Policy};
use minuta::llm::claude::{check, Claude};
use minuta::llm::Backend;
use minuta::tools::{run_capture, RunOpts};

const FAKE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bin/claude");

/// The fake is configured through the environment, which is process-wide: hold this for
/// the whole of a test that sets variables.
static ENV: Mutex<()> = Mutex::new(());

struct Env<'a> {
    _guard: MutexGuard<'a, ()>,
    keys: Vec<&'static str>,
}

impl Env<'_> {
    fn new() -> Env<'static> {
        Env {
            _guard: ENV.lock().unwrap_or_else(|p| p.into_inner()),
            keys: Vec::new(),
        }
    }

    fn set(&mut self, key: &'static str, value: impl AsRef<std::ffi::OsStr>) {
        std::env::set_var(key, value);
        self.keys.push(key);
    }
}

impl Drop for Env<'_> {
    fn drop(&mut self) {
        for k in &self.keys {
            std::env::remove_var(k);
        }
    }
}

fn sh(script: &str) -> Vec<OsString> {
    vec!["/bin/sh".into(), "-c".into(), script.into()]
}

fn claude(model: Option<&str>) -> Claude {
    Claude::new(PathBuf::from(FAKE), model.map(str::to_string))
}

fn lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn run_capture_round_trips_stdin() {
    let out = run_capture(
        &sh("cat"),
        b"hello \xff world",
        RunOpts::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(out, "hello \u{fffd} world");
}

#[test]
fn run_capture_large_stdin_does_not_deadlock() {
    let input = vec![b'x'; 4 * 1024 * 1024];
    let out = run_capture(&sh("cat"), &input, RunOpts::default(), &mut |_| {}).unwrap();
    assert_eq!(out.len(), input.len());
}

#[test]
fn run_capture_ignores_unread_stdin() {
    let input = vec![b'x'; 4 * 1024 * 1024];
    let out = run_capture(&sh("echo hi"), &input, RunOpts::default(), &mut |_| {}).unwrap();
    assert_eq!(out, "hi\n");
}

#[test]
fn run_capture_streams_stderr() {
    let mut seen = Vec::new();
    let out = run_capture(
        &sh("echo out; echo one >&2; echo two >&2"),
        b"",
        RunOpts::default(),
        &mut |l| seen.push(l.to_string()),
    )
    .unwrap();
    assert_eq!(out, "out\n");
    assert_eq!(seen, ["one", "two"]);
}

#[test]
fn run_capture_nonzero_is_tool_error() {
    let err = run_capture(
        &sh("echo boom >&2; exit 3"),
        b"",
        RunOpts::default(),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(
        matches!(&err, Error::Tool { code: Some(3), stderr, .. } if stderr == "boom"),
        "{err:?}"
    );
}

#[test]
fn run_capture_honors_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let opts = RunOpts {
        cwd: Some(dir.path().to_path_buf()),
        ..RunOpts::default()
    };
    let out = run_capture(&sh("pwd -P"), b"", opts, &mut |_| {}).unwrap();
    assert_eq!(
        out.trim(),
        fs::canonicalize(dir.path()).unwrap().to_str().unwrap()
    );
}

#[test]
fn run_capture_interrupt_kills_child() {
    let flag = Arc::new(AtomicBool::new(false));
    let setter = Arc::clone(&flag);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        setter.store(true, Ordering::SeqCst);
    });
    let opts = RunOpts {
        interrupt: Some(flag),
        ..RunOpts::default()
    };
    let start = Instant::now();
    let err = run_capture(&sh("exec sleep 30"), b"", opts, &mut |_| {}).unwrap_err();
    assert!(matches!(err, Error::Interrupted), "{err:?}");
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn claude_argv_flags() {
    let dir = tempfile::tempdir().unwrap();
    let argv = dir.path().join("argv");
    let mut env = Env::new();
    env.set("FAKE_CLAUDE_ARGV", &argv);
    claude(Some("sonnet"))
        .complete("be brief", "text", &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        lines(&argv),
        [
            "-p",
            "--output-format",
            "text",
            "--system-prompt=be brief",
            "--tools",
            "",
            "--no-session-persistence",
            "--setting-sources=local",
            "--strict-mcp-config",
            "--model=sonnet",
        ]
    );
}

#[test]
fn claude_argv_has_no_model_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let argv = dir.path().join("argv");
    let mut env = Env::new();
    env.set("FAKE_CLAUDE_ARGV", &argv);
    claude(None)
        .complete("s", "u", &AtomicBool::new(false))
        .unwrap();
    assert!(!lines(&argv).iter().any(|a| a.starts_with("--model")));
}

#[test]
fn claude_model_starting_with_dash_stays_one_argument() {
    let dir = tempfile::tempdir().unwrap();
    let argv = dir.path().join("argv");
    let mut env = Env::new();
    env.set("FAKE_CLAUDE_ARGV", &argv);
    claude(Some("--dangerously-skip-permissions"))
        .complete("s", "u", &AtomicBool::new(false))
        .unwrap();
    let got = lines(&argv);
    assert!(got.contains(&"--model=--dangerously-skip-permissions".to_string()));
    assert!(!got.contains(&"--dangerously-skip-permissions".to_string()));
}

#[test]
fn claude_input_on_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let stdin = dir.path().join("stdin");
    let mut env = Env::new();
    env.set("FAKE_CLAUDE_STDIN", &stdin);
    env.set("FAKE_CLAUDE_REPLY", "the notes");
    let out = claude(None)
        .complete("sys", "line one\nline two", &AtomicBool::new(false))
        .unwrap();
    assert_eq!(out.trim(), "the notes");
    assert_eq!(fs::read_to_string(&stdin).unwrap(), "line one\nline two");
}

#[test]
fn claude_runs_in_empty_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("cwd");
    let mut env = Env::new();
    env.set("FAKE_CLAUDE_CWD", &cwd);
    claude(None)
        .complete("s", "u", &AtomicBool::new(false))
        .unwrap();
    let got = lines(&cwd);
    assert_eq!(got.len(), 1, "working directory was not empty: {got:?}");
    let used = PathBuf::from(&got[0]);
    assert_ne!(
        used,
        fs::canonicalize(std::env::current_dir().unwrap()).unwrap()
    );
    assert!(!used.try_exists().unwrap(), "{used:?} was not removed");
}

#[test]
fn claude_nonzero_is_transient() {
    let mut env = Env::new();
    env.set("FAKE_CLAUDE_EXIT", "1");
    env.set("FAKE_CLAUDE_STDERR", "rate limited");
    let err = claude(None)
        .complete("s", "u", &AtomicBool::new(false))
        .unwrap_err();
    assert!(
        matches!(&err, Error::Llm { backend, message, transient: true }
            if backend == "claude" && message == "rate limited"),
        "{err:?}"
    );
}

#[test]
fn claude_empty_output_is_transient() {
    let mut env = Env::new();
    env.set("FAKE_CLAUDE_REPLY", "");
    let err = claude(Some("opus"))
        .complete("s", "u", &AtomicBool::new(false))
        .unwrap_err();
    assert!(
        matches!(&err, Error::Llm { backend, transient: true, .. } if backend == "claude:opus"),
        "{err:?}"
    );
}

#[test]
fn claude_interrupt_returns_interrupted() {
    let mut env = Env::new();
    env.set("FAKE_CLAUDE_HANG", "1");
    let flag = Arc::new(AtomicBool::new(false));
    let setter = Arc::clone(&flag);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        setter.store(true, Ordering::SeqCst);
    });
    let start = Instant::now();
    let err = claude(None).complete("s", "u", &flag).unwrap_err();
    assert!(matches!(err, Error::Interrupted), "{err:?}");
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn claude_shell_metacharacters_inert() {
    let dir = tempfile::tempdir().unwrap();
    let argv = dir.path().join("argv");
    let cwd = dir.path().join("cwd");
    let mut env = Env::new();
    env.set("FAKE_CLAUDE_ARGV", &argv);
    env.set("FAKE_CLAUDE_CWD", &cwd);
    let system = "$(touch pwned) `touch pwned` \"quoted\" 'single'; touch pwned";
    claude(None)
        .complete(system, "$(touch pwned)", &AtomicBool::new(false))
        .unwrap();
    assert!(lines(&argv).contains(&format!("--system-prompt={system}")));
    // The fake lists its working directory before exiting; nothing may have been created.
    assert!(!lines(&cwd).iter().any(|n| n == "pwned"));
    assert!(!Path::new("pwned").try_exists().unwrap());
}

#[test]
fn claude_scratch_dir_removed_after_nonzero_exit() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("cwd");
    let mut env = Env::new();
    env.set("FAKE_CLAUDE_CWD", &cwd);
    env.set("FAKE_CLAUDE_EXIT", "2");
    claude(None)
        .complete("s", "u", &AtomicBool::new(false))
        .unwrap_err();
    assert!(!PathBuf::from(&lines(&cwd)[0]).try_exists().unwrap());
}

#[test]
fn claude_scratch_dir_removed_after_interrupt() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("cwd");
    let mut env = Env::new();
    env.set("FAKE_CLAUDE_CWD", &cwd);
    env.set("FAKE_CLAUDE_HANG", "1");
    let flag = Arc::new(AtomicBool::new(false));
    let setter = Arc::clone(&flag);
    let watched = cwd.clone();
    std::thread::spawn(move || {
        // Interrupt only once the fake is running and has recorded its directory.
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if fs::read_to_string(&watched).is_ok_and(|t| t.lines().next().is_some()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        setter.store(true, Ordering::SeqCst);
    });
    let err = claude(None).complete("s", "u", &flag).unwrap_err();
    assert!(matches!(err, Error::Interrupted), "{err:?}");
    assert!(!PathBuf::from(&lines(&cwd)[0]).try_exists().unwrap());
}

/// Two attempts and no backoff: a transient failure is retried at once.
const RETRY_ONCE: Policy = Policy {
    retries: 2,
    backoff: Duration::ZERO,
};

const POLL: Duration = Duration::from_millis(1);

/// Polls `path` until it holds a complete first line, and returns that line; panics after
/// a generous deadline.
fn wait_for_first_line(path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match fs::read_to_string(path) {
            Ok(text) => {
                if let Some((first, _)) = text.split_once('\n') {
                    return first.to_string();
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => panic!("{}: {e}", path.display()),
        }
        assert!(
            Instant::now() < deadline,
            "{} never got a line",
            path.display()
        );
        std::thread::sleep(POLL);
    }
}

/// Polls until the directory named on the first line of `cwd_file` (written by the fake)
/// is gone; panics after a generous deadline.
fn wait_for_removed_scratch(cwd_file: &Path) {
    let scratch = PathBuf::from(wait_for_first_line(cwd_file));
    let deadline = Instant::now() + Duration::from_secs(30);
    while scratch.try_exists().unwrap() {
        assert!(Instant::now() < deadline, "{scratch:?} was never removed");
        std::thread::sleep(POLL);
    }
}

// The terminal's Ctrl-C reaches `claude` (same process group) before the handler sets the
// flag: the fake dies from a signal (KILL: unlike INT, a parent cannot make it ignored),
// and the flag is set only after `complete` has seen the death (it removes its scratch
// directory right after the process is reaped).
#[test]
fn claude_signal_death_then_interrupt_does_not_retry() {
    let dir = tempfile::tempdir().unwrap();
    let calls = dir.path().join("calls");
    let cwd = dir.path().join("cwd");
    let mut env = Env::new();
    env.set("FAKE_CLAUDE_CALLS", &calls);
    env.set("FAKE_CLAUDE_CWD", &cwd);
    env.set("FAKE_CLAUDE_SIGNAL", "KILL");
    let flag = Arc::new(AtomicBool::new(false));
    let setter = Arc::clone(&flag);
    let watched = cwd.clone();
    let handler = std::thread::spawn(move || {
        wait_for_removed_scratch(&watched);
        // A late handler. This delay never orders anything the test relies on: with the
        // grace below, any delay under 30 s passes. It only gives a regression time to
        // start the retry, so that it shows as a second call.
        std::thread::sleep(Duration::from_millis(200));
        setter.store(true, Ordering::SeqCst);
    });
    let cache = CallCache::new(dir.path());
    let backend = claude(None).with_signal_grace(Duration::from_secs(30));
    let err = call(&backend, &cache, &RETRY_ONCE, "s", "u", true, &flag).unwrap_err();
    handler.join().unwrap();
    assert!(matches!(err, Error::Interrupted), "{err:?}");
    assert_eq!(lines(&calls), ["call"]);
}

#[test]
fn claude_signal_death_without_interrupt_is_transient() {
    let dir = tempfile::tempdir().unwrap();
    let calls = dir.path().join("calls");
    let mut env = Env::new();
    env.set("FAKE_CLAUDE_CALLS", &calls);
    env.set("FAKE_CLAUDE_SIGNAL", "KILL");
    let cache = CallCache::new(dir.path());
    let err = call(
        &claude(None),
        &cache,
        &RETRY_ONCE,
        "s",
        "u",
        true,
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(
        matches!(&err, Error::Llm { backend, message, transient: true }
            if backend == "claude" && message == "exited with status none"),
        "{err:?}"
    );
    assert_eq!(lines(&calls), ["call", "call"]);
}

#[test]
fn claude_nonzero_with_empty_stderr_uses_status_message() {
    let mut env = Env::new();
    env.set("FAKE_CLAUDE_EXIT", "7");
    let err = claude(None)
        .complete("s", "u", &AtomicBool::new(false))
        .unwrap_err();
    assert!(
        matches!(&err, Error::Llm { message, transient: true, .. }
            if message == "exited with status 7"),
        "{err:?}"
    );
}

#[test]
fn claude_reports_id_and_context() {
    assert_eq!(claude(None).id(), "claude");
    assert_eq!(claude(Some("sonnet")).id(), "claude:sonnet");
    assert_eq!(claude(None).context_tokens(), 150_000);
}

#[test]
fn check_finds_configured_binary_and_version() {
    let info = check(FAKE).unwrap();
    assert_eq!(info.path, PathBuf::from(FAKE));
    assert_eq!(info.version, "2.0.0-fake (Claude Code)");
}

#[test]
fn check_missing_claude_hints_install() {
    let missing = check("minuta-no-such-claude-binary").unwrap_err();
    assert_eq!(missing.name, "claude");
    assert_eq!(
        missing.hint,
        "install Claude Code: https://claude.com/claude-code"
    );
}

#[test]
fn check_missing_configured_path_is_missing() {
    let missing = check("/nonexistent/dir/claude").unwrap_err();
    assert_eq!(missing.name, "claude");
    assert!(missing.hint.contains("/nonexistent/dir/claude"));
}
