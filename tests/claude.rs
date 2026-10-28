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

/// Polls `cond` under a generous deadline; panics with `what` if it never holds.
fn poll_until(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !cond() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Whether `pid` exists; any `kill(pid, 0)` error other than "no such process" panics.
fn pid_alive(pid: u32) -> bool {
    let pid = libc::pid_t::try_from(pid).unwrap();
    // SAFETY: signal 0 only checks that the process exists.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    let err = std::io::Error::last_os_error();
    assert_eq!(
        err.raw_os_error(),
        Some(libc::ESRCH),
        "kill -0 {pid}: {err}"
    );
    false
}

/// On drop, sets the interrupt flag and, while the child is not yet reaped, kills its
/// process group, so a failing test does not leave it hanging after the binary exits.
struct StopOnDrop {
    flag: Arc<AtomicBool>,
    pid: Option<u32>,
}

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.flag.store(true, Ordering::SeqCst);
        if let Some(pid) = self.pid.and_then(|p| libc::pid_t::try_from(p).ok()) {
            // SAFETY: the child leads its own group and is not reaped, so neither id
            // can name another process. Errors are ignored: it may already be dead.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

/// The pid in `path`, or `None` while the file does not exist yet. The writer renames
/// it into place, so an existing file is complete.
fn read_pid(path: &Path) -> Option<u32> {
    match fs::read_to_string(path) {
        Ok(t) => Some(
            t.trim()
                .parse()
                .unwrap_or_else(|e| panic!("{}: {e}", path.display())),
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => panic!("{}: {e}", path.display()),
    }
}

/// Joins `worker`, re-raising its panic.
fn join<T>(worker: std::thread::JoinHandle<T>) -> T {
    worker
        .join()
        .unwrap_or_else(|e| std::panic::resume_unwind(e))
}

/// Runs `run` on a worker thread and sets its interrupt flag only once the child it
/// starts has written its pid to `pid_file`. The child must hang far longer than the
/// poll deadline, so `run` returning within it shows the child was killed, not that it
/// finished. Returns `run`'s result after checking that the child is gone.
fn interrupt_when_started<T: Send + std::fmt::Debug + 'static>(
    pid_file: &Path,
    run: impl FnOnce(Arc<AtomicBool>) -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    let flag = Arc::new(AtomicBool::new(false));
    let mut guard = StopOnDrop {
        flag: Arc::clone(&flag),
        pid: None,
    };
    let worker = {
        let flag = Arc::clone(&flag);
        std::thread::spawn(move || run(flag))
    };
    let mut pid = None;
    poll_until("the child never wrote its pid", || {
        pid = read_pid(pid_file);
        pid.is_some() || worker.is_finished()
    });
    let Some(pid) = pid else {
        panic!("run ended before the child started: {:?}", join(worker));
    };
    guard.pid = Some(pid);
    let alive = pid_alive(pid);
    flag.store(true, Ordering::SeqCst);
    poll_until("run did not return after the interrupt", || {
        worker.is_finished()
    });
    let res = join(worker);
    // `run` has reaped the child: its pid may be reused from here on.
    guard.pid = None;
    assert!(alive, "child {pid} exited before the interrupt: {res:?}");
    poll_until(&format!("child {pid} is still alive"), || !pid_alive(pid));
    res
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
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let script = format!(
        "echo $$ > '{0}.tmp' && mv '{0}.tmp' '{0}' && exec sleep 300",
        pid_file.display()
    );
    let res = interrupt_when_started(&pid_file, move |flag| {
        let opts = RunOpts {
            interrupt: Some(flag),
            ..RunOpts::default()
        };
        run_capture(&sh(&script), b"", opts, &mut |_| {})
    });
    assert!(matches!(res, Err(Error::Interrupted)), "{res:?}");
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
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let mut env = Env::new();
    env.set("FAKE_CLAUDE_HANG", "1");
    env.set("FAKE_CLAUDE_PID", &pid_file);
    let res = interrupt_when_started(&pid_file, |flag| claude(None).complete("s", "u", &flag));
    assert!(matches!(res, Err(Error::Interrupted)), "{res:?}");
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
    let pid_file = dir.path().join("pid");
    let mut env = Env::new();
    env.set("FAKE_CLAUDE_CWD", &cwd);
    env.set("FAKE_CLAUDE_HANG", "1");
    // The fake records its directory before its pid, so the interrupt comes after both.
    env.set("FAKE_CLAUDE_PID", &pid_file);
    let res = interrupt_when_started(&pid_file, |flag| claude(None).complete("s", "u", &flag));
    assert!(matches!(res, Err(Error::Interrupted)), "{res:?}");
    assert!(!PathBuf::from(&lines(&cwd)[0]).try_exists().unwrap());
}

/// Two attempts and no backoff: a transient failure is retried at once.
const RETRY_ONCE: Policy = Policy {
    retries: 2,
    backoff: Duration::ZERO,
};

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
