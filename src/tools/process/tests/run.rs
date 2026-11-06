//! `run` and `run_capture` on real child processes: argv handling, streaming, the stderr
//! tail and interrupts.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::Instant;

use super::*;
use crate::test_support::{fixture, retry_busy};

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn run_r(argv: &[OsString], opts: &RunOpts, on_line: &mut dyn FnMut(&str)) -> Result<()> {
    retry_busy(|| run(argv, opts.clone(), on_line))
}

#[test]
fn argv_with_shell_metacharacters_is_safe() {
    let dir = tempfile::tempdir().unwrap();
    let echo = script(dir.path(), "echo1", r#"printf '%s\n' "$1""#);
    let name = OsString::from(r#"a b"$(touch pwned)'.mp4"#);
    let opts = RunOpts {
        cwd: Some(dir.path().to_path_buf()),
        ..RunOpts::default()
    };
    let mut lines = Vec::new();
    run_r(&[echo.into_os_string(), name.clone()], &opts, &mut |l| {
        lines.push(l.to_string())
    })
    .unwrap();
    assert_eq!(lines, vec![name.to_string_lossy().into_owned()]);
    assert!(!dir.path().join("pwned").exists());
}

#[test]
fn streams_lines() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(dir.path(), "both", "echo out1\necho err1 >&2\necho out2").into_os_string();
    let mut lines = Vec::new();
    run_r(&[bin], &RunOpts::default(), &mut |l| {
        lines.push(l.to_string())
    })
    .unwrap();
    lines.sort();
    assert_eq!(lines, vec!["err1", "out1", "out2"]);
}

#[test]
fn nonzero_exit_is_tool_error() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(dir.path(), "bad", "echo noise\necho boom >&2\nexit 3").into_os_string();
    let err = run_r(&[bin], &RunOpts::default(), &mut |_| {}).unwrap_err();
    match err {
        Error::Tool { code, stderr, .. } => {
            assert_eq!(code, Some(3));
            assert!(stderr.contains("boom"), "stderr was {stderr:?}");
            assert!(!stderr.contains("noise"), "stderr was {stderr:?}");
        }
        other => panic!("expected Tool error, got {other:?}"),
    }
}

#[test]
fn stderr_keeps_only_last_20_lines() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(
        dir.path(),
        "noisy",
        "i=1\nwhile [ $i -le 25 ]; do echo line$i >&2; i=$((i+1)); done\nexit 1",
    )
    .into_os_string();
    let err = run_r(&[bin], &RunOpts::default(), &mut |_| {}).unwrap_err();
    match err {
        Error::Tool { stderr, .. } => {
            let expected: Vec<String> = (6..=25).map(|i| format!("line{i}")).collect();
            assert_eq!(stderr, expected.join("\n"));
        }
        other => panic!("expected Tool error, got {other:?}"),
    }
}

fn interrupt_after(ms: u64) -> RunOpts {
    let flag = Arc::new(AtomicBool::new(false));
    let setter = Arc::clone(&flag);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms));
        setter.store(true, Ordering::SeqCst);
    });
    RunOpts {
        interrupt: Some(flag),
        ..RunOpts::default()
    }
}

#[test]
fn interrupt_kills_child() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(dir.path(), "slow", "exec sleep 30").into_os_string();
    let opts = interrupt_after(100);
    let start = Instant::now();
    let err = run_r(&[bin], &opts, &mut |_| {}).unwrap_err();
    assert!(matches!(err, Error::Interrupted), "got {err:?}");
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn interrupt_kills_child_that_closed_stdio() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(dir.path(), "quiet", "exec >/dev/null 2>&1\nexec sleep 30").into_os_string();
    // Late enough that the pipes are already closed when the flag is set.
    let opts = interrupt_after(400);
    let start = Instant::now();
    let err = run_r(&[bin], &opts, &mut |_| {}).unwrap_err();
    assert!(matches!(err, Error::Interrupted), "got {err:?}");
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn invalid_utf8_line_does_not_stop_streaming() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(
        dir.path(),
        "latin1",
        r"printf 'bad \377\376 bytes\r\n'; echo after1; echo after3; echo after2 >&2",
    )
    .into_os_string();
    let mut lines = Vec::new();
    run_r(&[bin], &RunOpts::default(), &mut |l| {
        lines.push(l.to_string())
    })
    .unwrap();
    // Only stdout order is fixed; stderr is read by a separate thread.
    let stdout: Vec<_> = lines.iter().filter(|l| *l != "after2").collect();
    assert_eq!(
        stdout,
        vec!["bad \u{fffd}\u{fffd} bytes", "after1", "after3"]
    );
    assert!(lines.contains(&"after2".to_string()));
}

#[test]
fn spawn_failure_names_the_program() {
    let dir = tempfile::tempdir().unwrap();
    let err = run(
        &[dir.path().as_os_str().to_owned()],
        RunOpts::default(),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(err, Error::Io(_)), "got {err:?}");
    assert!(
        err.to_string().contains(&dir.path().display().to_string()),
        "message was {err}"
    );
}

#[test]
fn pump_splits_carriage_returns() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(dir.path(), "cr", r"printf 'one\rtwo\r\nthree\nfour\rfive'").into_os_string();
    let mut lines = Vec::new();
    run_r(&[bin], &RunOpts::default(), &mut |l| {
        lines.push(l.to_string())
    })
    .unwrap();
    assert_eq!(lines, vec!["one", "two", "three", "four", "five"]);
}

#[test]
fn carriage_return_update_arrives_while_tool_runs() {
    let dir = tempfile::tempdir().unwrap();
    let go = dir.path().join("go");
    // The tool only finishes once the test has seen its first update, so the update must
    // reach `on_line` while the tool is still running.
    let bin = script(
        dir.path(),
        "live",
        r#"printf 'waiting\r'
i=0
while [ ! -e "$1" ] && [ $i -lt 600 ]; do sleep 0.05; i=$((i+1)); done
if [ -e "$1" ]; then echo done; else echo 'timed out waiting for the test' >&2; exit 1; fi"#,
    )
    .into_os_string();
    let mut lines = Vec::new();
    run_r(
        &[bin, go.clone().into_os_string()],
        &RunOpts::default(),
        &mut |l| {
            if l == "waiting" {
                fs::write(&go, b"").unwrap();
            }
            lines.push(l.to_string());
        },
    )
    .unwrap();
    assert_eq!(lines, vec!["waiting", "done"]);
}

#[test]
fn stderr_tail_keeps_only_latest_carriage_return_update() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(
        dir.path(),
        "stats",
        r"printf 'begin\r\n' >&2
printf 'p1\rp2\rp3\r' >&2
printf 'error: boom\n' >&2
exit 1",
    )
    .into_os_string();
    match run_r(&[bin], &RunOpts::default(), &mut |_| {}).unwrap_err() {
        Error::Tool { stderr, .. } => assert_eq!(stderr, "begin\np3\nerror: boom"),
        other => panic!("expected Tool error, got {other:?}"),
    }
}

#[test]
fn progress_lines_kept_out_of_tail() {
    let bin = fixture("tools/whisper-fails-late").into_os_string();
    match run_r(&[bin], &RunOpts::default(), &mut |_| {}).unwrap_err() {
        Error::Tool { tool, code, stderr } => {
            assert_eq!(tool, "whisper-fails-late");
            assert_eq!(code, Some(1));
            assert_eq!(
                stderr,
                "whisper_init_from_file_with_params_no_state: loading model\n\
                 whisper_model_load: model size    =   77.11 MB\n\
                 whisper_full_with_state: failed to decode"
            );
        }
        other => panic!("expected Tool error, got {other:?}"),
    }
}

#[test]
fn tool_error_names_the_tool() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(dir.path(), "my-tool", "exit 4").into_os_string();
    match run_r(&[bin], &RunOpts::default(), &mut |_| {}).unwrap_err() {
        Error::Tool { tool, code, stderr } => {
            assert_eq!(tool, "my-tool");
            assert_eq!(code, Some(4));
            assert_eq!(stderr, "");
        }
        other => panic!("expected Tool error, got {other:?}"),
    }
}

#[test]
fn run_rejects_empty_argv() {
    match run(&[], RunOpts::default(), &mut |_| {}).unwrap_err() {
        Error::Usage(m) => assert_eq!(m, "empty command"),
        other => panic!("expected Usage error, got {other:?}"),
    }
}

#[test]
fn run_capture_rejects_empty_argv() {
    match run_capture(&[], b"", RunOpts::default(), &mut |_| {}).unwrap_err() {
        Error::Usage(m) => assert_eq!(m, "empty command"),
        other => panic!("expected Usage error, got {other:?}"),
    }
}

/// Whether `pid` is still a live (not zombie) process, asked of `ps`.
fn alive(pid: &str) -> bool {
    let argv = ["ps", "-o", "stat=", "-p", pid].map(OsString::from);
    let mut stat = String::new();
    match run(&argv, RunOpts::default(), &mut |l| stat.push_str(l.trim())) {
        Ok(_) => !stat.starts_with('Z'),
        // `ps` exits non-zero when no such process exists.
        Err(Error::Tool { .. }) => false,
        Err(e) => panic!("cannot run ps: {e}"),
    }
}

#[test]
fn kill_reaches_grandchild() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("grandchild.pid");
    let flag = Arc::new(AtomicBool::new(false));
    // Interrupts once the grandchild is running, which the fixture signals by renaming
    // its pid file into place.
    let watcher = {
        let (flag, pid_file) = (Arc::clone(&flag), pid_file.clone());
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(30);
            let pid = loop {
                match fs::read_to_string(&pid_file) {
                    Ok(pid) => break Some(pid.trim().to_string()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => panic!("cannot read {}: {e}", pid_file.display()),
                }
                if Instant::now() >= deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(25));
            };
            flag.store(true, Ordering::SeqCst);
            pid
        })
    };
    let argv = [
        fixture("tools/spawn-grandchild").into_os_string(),
        pid_file.into_os_string(),
    ];
    let opts = RunOpts {
        interrupt: Some(flag),
        ..RunOpts::default()
    };
    let err = run(&argv, opts, &mut |_| {}).unwrap_err();
    let pid = watcher
        .join()
        .unwrap()
        .expect("the fixture never wrote the grandchild's pid");
    assert!(matches!(err, Error::Interrupted), "got {err:?}");

    let deadline = Instant::now() + Duration::from_secs(20);
    while alive(&pid) {
        if Instant::now() >= deadline {
            let kill = ["kill", "-KILL", pid.as_str()].map(OsString::from);
            // Best effort: only stops the leaked sleep before failing.
            let _ = run(&kill, RunOpts::default(), &mut |_| {});
            panic!("grandchild {pid} survived the kill");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
