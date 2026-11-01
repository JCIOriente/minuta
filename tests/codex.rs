#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use minuta::error::Error;
use minuta::llm::call::{call, CallCache, Policy};
use minuta::llm::codex::{check, Codex};
use minuta::llm::Backend;

const FAKE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bin/codex");

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

fn codex(model: Option<&str>) -> Codex {
    Codex::new(PathBuf::from(FAKE), model.map(str::to_string))
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
fn codex_argv_flags() {
    let dir = tempfile::tempdir().unwrap();
    let argv = dir.path().join("argv");
    let mut env = Env::new();
    env.set("FAKE_CODEX_ARGV", &argv);
    codex(Some("gpt-x"))
        .complete("be brief", "text", &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        lines(&argv),
        [
            "exec",
            "--ephemeral",
            "--skip-git-repo-check",
            "--ignore-user-config",
            "--ignore-rules",
            "--sandbox=read-only",
            "--model=gpt-x",
            "--config=developer_instructions=\"be brief\"",
            "-",
        ]
    );
}

#[test]
fn codex_argv_has_no_model_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let argv = dir.path().join("argv");
    let mut env = Env::new();
    env.set("FAKE_CODEX_ARGV", &argv);
    codex(None)
        .complete("s", "u", &AtomicBool::new(false))
        .unwrap();
    assert!(!lines(&argv).iter().any(|a| a.starts_with("--model")));
}

#[test]
fn codex_model_starting_with_dash_stays_one_argument() {
    let dir = tempfile::tempdir().unwrap();
    let argv = dir.path().join("argv");
    let mut env = Env::new();
    env.set("FAKE_CODEX_ARGV", &argv);
    codex(Some("--dangerously-skip-permissions"))
        .complete("s", "u", &AtomicBool::new(false))
        .unwrap();
    let got = lines(&argv);
    assert!(got.contains(&"--model=--dangerously-skip-permissions".to_string()));
    assert!(!got.contains(&"--dangerously-skip-permissions".to_string()));
}

#[test]
fn codex_input_on_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let stdin = dir.path().join("stdin");
    let mut env = Env::new();
    env.set("FAKE_CODEX_STDIN", &stdin);
    env.set("FAKE_CODEX_REPLY", "the notes");
    let out = codex(None)
        .complete("sys", "line one\nline two", &AtomicBool::new(false))
        .unwrap();
    assert_eq!(out.trim(), "the notes");
    assert_eq!(fs::read_to_string(&stdin).unwrap(), "line one\nline two");
}

#[test]
fn codex_runs_in_empty_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("cwd");
    let mut env = Env::new();
    env.set("FAKE_CODEX_CWD", &cwd);
    codex(None)
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
fn codex_nonzero_is_transient() {
    let mut env = Env::new();
    env.set("FAKE_CODEX_EXIT", "1");
    env.set("FAKE_CODEX_STDERR", "rate limited");
    let err = codex(None)
        .complete("s", "u", &AtomicBool::new(false))
        .unwrap_err();
    assert!(
        matches!(&err, Error::Llm { backend, message, transient: true }
            if backend == "codex" && message == "rate limited"),
        "{err:?}"
    );
}

#[test]
fn codex_empty_output_is_transient() {
    let mut env = Env::new();
    env.set("FAKE_CODEX_REPLY", "");
    let err = codex(Some("opus"))
        .complete("s", "u", &AtomicBool::new(false))
        .unwrap_err();
    assert!(
        matches!(&err, Error::Llm { backend, transient: true, .. } if backend == "codex:opus"),
        "{err:?}"
    );
}

#[test]
fn codex_interrupt_returns_interrupted() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let mut env = Env::new();
    env.set("FAKE_CODEX_HANG", "1");
    env.set("FAKE_CODEX_PID", &pid_file);
    let res = interrupt_when_started(&pid_file, |flag| codex(None).complete("s", "u", &flag));
    assert!(matches!(res, Err(Error::Interrupted)), "{res:?}");
}

#[test]
fn codex_shell_metacharacters_inert() {
    let dir = tempfile::tempdir().unwrap();
    let argv = dir.path().join("argv");
    let cwd = dir.path().join("cwd");
    let mut env = Env::new();
    env.set("FAKE_CODEX_ARGV", &argv);
    env.set("FAKE_CODEX_CWD", &cwd);
    let system = "$(touch pwned) `touch pwned` \"quoted\" 'single'; touch pwned";
    codex(None)
        .complete(system, "$(touch pwned)", &AtomicBool::new(false))
        .unwrap();
    assert!(lines(&argv).contains(&format!(
        "--config=developer_instructions=\"{}\"",
        "$(touch pwned) `touch pwned` \\\"quoted\\\" 'single'; touch pwned"
    )));
    // The fake lists its working directory before exiting; nothing may have been created.
    assert!(!lines(&cwd).iter().any(|n| n == "pwned"));
    assert!(!Path::new("pwned").try_exists().unwrap());
}

#[test]
fn codex_scratch_dir_removed_after_nonzero_exit() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("cwd");
    let mut env = Env::new();
    env.set("FAKE_CODEX_CWD", &cwd);
    env.set("FAKE_CODEX_EXIT", "2");
    codex(None)
        .complete("s", "u", &AtomicBool::new(false))
        .unwrap_err();
    assert!(!PathBuf::from(&lines(&cwd)[0]).try_exists().unwrap());
}

#[test]
fn codex_scratch_dir_removed_after_interrupt() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("cwd");
    let pid_file = dir.path().join("pid");
    let mut env = Env::new();
    env.set("FAKE_CODEX_CWD", &cwd);
    env.set("FAKE_CODEX_HANG", "1");
    // The fake records its directory before its pid, so the interrupt comes after both.
    env.set("FAKE_CODEX_PID", &pid_file);
    let res = interrupt_when_started(&pid_file, |flag| codex(None).complete("s", "u", &flag));
    assert!(matches!(res, Err(Error::Interrupted)), "{res:?}");
    assert!(!PathBuf::from(&lines(&cwd)[0]).try_exists().unwrap());
}

/// Two attempts and no backoff: a transient failure is retried at once.
const RETRY_ONCE: Policy = Policy {
    retries: 2,
    backoff: Duration::ZERO,
};

#[test]
fn codex_signal_death_without_interrupt_is_transient() {
    let dir = tempfile::tempdir().unwrap();
    let calls = dir.path().join("calls");
    let mut env = Env::new();
    env.set("FAKE_CODEX_CALLS", &calls);
    env.set("FAKE_CODEX_SIGNAL", "KILL");
    let cache = CallCache::new(dir.path());
    let err = call(
        &codex(None),
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
            if backend == "codex" && message == "exited with status none"),
        "{err:?}"
    );
    assert_eq!(lines(&calls), ["call", "call"]);
}

#[test]
fn codex_nonzero_with_empty_stderr_uses_status_message() {
    let mut env = Env::new();
    env.set("FAKE_CODEX_EXIT", "7");
    let err = codex(None)
        .complete("s", "u", &AtomicBool::new(false))
        .unwrap_err();
    assert!(
        matches!(&err, Error::Llm { message, transient: true, .. }
            if message == "exited with status 7"),
        "{err:?}"
    );
}

#[test]
fn codex_reports_id_and_context() {
    assert_eq!(codex(None).id(), "codex");
    assert_eq!(codex(Some("gpt-x")).id(), "codex:gpt-x");
    assert_eq!(codex(None).context_tokens(), 128_000);
}

#[test]
fn check_finds_configured_binary_and_version() {
    let info = check(FAKE).unwrap();
    assert_eq!(info.path, PathBuf::from(FAKE));
    assert_eq!(info.version, "codex-cli 0.0.0-fake");
}

#[test]
fn check_missing_codex_hints_install() {
    let missing = check("minuta-no-such-codex-binary").unwrap_err();
    assert_eq!(missing.name, "codex");
    assert_eq!(missing.hint, "install Codex: npm install -g @openai/codex");
}

#[test]
fn check_missing_configured_path_is_missing() {
    let missing = check("/nonexistent/dir/codex").unwrap_err();
    assert_eq!(missing.name, "codex");
    assert!(missing.hint.contains("/nonexistent/dir/codex"));
}

#[test]
fn codex_system_prompt_is_a_toml_string() {
    let dir = tempfile::tempdir().unwrap();
    let argv = dir.path().join("argv");
    let mut env = Env::new();
    env.set("FAKE_CODEX_ARGV", &argv);
    codex(None)
        .complete(
            "say \"hi\"\\ now\nline two\ttab\u{1}",
            "u",
            &AtomicBool::new(false),
        )
        .unwrap();
    let got = lines(&argv);
    // The newline escape keeps the argument on one line of the recorded argv.
    assert!(
        got.contains(
            &"--config=developer_instructions=\"say \\\"hi\\\"\\\\ now\\nline two\\ttab\\u0001\""
                .to_string()
        ),
        "{got:?}"
    );
}
