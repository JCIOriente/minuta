#![cfg(unix)]

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use minuta::error::{Error, Result};
use minuta::tools::{
    inspect, locate_tool, preflight, run, run_capture, Output, Report, RunOpts, ToolInfo,
    ToolPaths, Toolchain,
};

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Retry while a freshly written script is still held open by a concurrent fork in
/// another test thread (ETXTBSY on Linux).
fn retry<T>(mut f: impl FnMut() -> Result<T>) -> Result<T> {
    let mut attempts = 0;
    loop {
        let res = f();
        let busy = match &res {
            Err(Error::Io(e)) => e.to_string().contains("Text file busy"),
            Err(Error::MissingTools(m)) => busy_hint(m.iter().map(|m| m.hint.as_str())),
            _ => false,
        };
        attempts += 1;
        if !busy || attempts >= 20 {
            return res;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn busy_hint<'a>(mut hints: impl Iterator<Item = &'a str>) -> bool {
    hints.any(|h| h.contains("Text file busy"))
}

fn inspect_r(paths: &ToolPaths, need_model: bool) -> Report {
    for _ in 0..20 {
        let report = inspect(paths, need_model);
        if !busy_hint(report.missing.iter().map(|m| m.hint.as_str())) {
            return report;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    inspect(paths, need_model)
}

fn run_r(argv: &[OsString], opts: &RunOpts, on_line: &mut dyn FnMut(&str)) -> Result<Output> {
    retry(|| run(argv, opts.clone(), on_line))
}

fn preflight_r(paths: &ToolPaths, need_model: bool) -> Result<Toolchain> {
    retry(|| preflight(paths, need_model))
}

fn missing_of(err: Error) -> Vec<(String, String)> {
    match err {
        Error::MissingTools(m) => m.into_iter().map(|m| (m.name, m.hint)).collect(),
        other => panic!("expected MissingTools, got {other:?}"),
    }
}

/// A dir holding working fake ffmpeg, ffprobe and whisper-cli plus a model file.
fn fake_toolchain(dir: &Path) -> ToolPaths {
    script(dir, "ffmpeg", "echo 'ffmpeg version 7.1'");
    script(dir, "ffprobe", "echo 'ffprobe version 7.1'");
    script(dir, "whisper-cli", "echo 'whisper 1.7.2'");
    let model = dir.join("ggml-base.bin");
    fs::write(&model, b"x").unwrap();
    ToolPaths {
        path: Some(dir.as_os_str().to_owned()),
        model: Some(model),
        ..ToolPaths::default()
    }
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
    let out = run_r(&[bin], &RunOpts::default(), &mut |l| {
        lines.push(l.to_string())
    })
    .unwrap();
    assert!(out.status.success());
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
fn preflight_lists_all_missing() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ToolPaths {
        path: Some(dir.path().as_os_str().to_owned()),
        ..ToolPaths::default()
    };
    let missing = missing_of(preflight(&paths, true).unwrap_err());
    let names: Vec<_> = missing.iter().map(|m| m.0.as_str()).collect();
    assert_eq!(names, vec!["ffmpeg", "ffprobe", "whisper-cli", "model"]);
    assert!(missing.iter().all(|m| !m.1.is_empty()));
    // The model is only required when asked for.
    assert_eq!(missing_of(preflight(&paths, false).unwrap_err()).len(), 3);
}

#[test]
fn version_captured() {
    let dir = tempfile::tempdir().unwrap();
    let paths = fake_toolchain(dir.path());
    script(
        dir.path(),
        "ffmpeg",
        "echo 'ffmpeg version 7.1 extra'\necho second",
    );
    let tc = preflight_r(&paths, true).unwrap();
    assert_eq!(tc.ffmpeg.version, "ffmpeg version 7.1 extra");
    assert_eq!(tc.ffprobe.version, "ffprobe version 7.1");
    assert_eq!(tc.whisper.version, "whisper 1.7.2");
    assert_eq!(tc.ffmpeg.path, dir.path().join("ffmpeg"));
    assert_eq!(tc.model, paths.model);
}

#[test]
fn empty_and_relative_path_entries_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let evil = tempfile::tempdir().unwrap();
    script(evil.path(), "ffmpeg", "echo evil");
    let cwd_guard = std::env::current_dir().unwrap();
    // A relative entry that resolves to a real executable from the current directory.
    let rel = pathdiff(evil.path(), &cwd_guard);
    let search = std::env::join_paths([PathBuf::new(), rel, dir.path().to_path_buf()]).unwrap();
    let paths = ToolPaths {
        path: Some(search),
        ..ToolPaths::default()
    };
    let report = inspect_r(&paths, false);
    assert!(report.ffmpeg.is_none());
    assert!(report.missing.iter().any(|m| m.name == "ffmpeg"));
}

/// Relative path from `base` to `target`, both absolute.
fn pathdiff(target: &Path, base: &Path) -> PathBuf {
    let (target, base) = (target.canonicalize().unwrap(), base.canonicalize().unwrap());
    let t: Vec<_> = target.components().collect();
    let b: Vec<_> = base.components().collect();
    let common = t.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut out = PathBuf::new();
    for _ in common..b.len() {
        out.push("..");
    }
    for c in &t[common..] {
        out.push(c);
    }
    out
}

#[test]
fn failing_version_probe_is_reported_missing() {
    let dir = tempfile::tempdir().unwrap();
    let paths = fake_toolchain(dir.path());
    script(dir.path(), "ffprobe", "echo 'some text'\nexit 1");
    let report = inspect_r(&paths, true);
    // Doctor data still shows where the tool was found.
    assert_eq!(report.ffprobe.unwrap().path, dir.path().join("ffprobe"));
    assert_eq!(report.missing.len(), 1);
    let m = &report.missing[0];
    assert_eq!(m.name, "ffprobe");
    assert!(m.hint.contains("found at"), "hint was {:?}", m.hint);
    assert!(m.hint.contains("--version") || m.hint.contains("-version"));
    assert!(matches!(
        preflight_r(&paths, true).unwrap_err(),
        Error::MissingTools(_)
    ));
}

#[test]
fn silent_version_probe_is_reported_missing() {
    let dir = tempfile::tempdir().unwrap();
    let paths = fake_toolchain(dir.path());
    script(dir.path(), "whisper-cli", "exit 0");
    let missing = missing_of(preflight_r(&paths, true).unwrap_err());
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].0, "whisper-cli");
    assert!(missing[0].1.contains("found at"));
}

#[test]
fn configured_bad_paths_get_specific_hints() {
    let dir = tempfile::tempdir().unwrap();
    let mut paths = fake_toolchain(dir.path());
    let bad = dir.path().join("nope");
    paths.ffmpeg = Some(bad.clone());
    paths.model = Some(dir.path().to_path_buf());
    let missing = missing_of(preflight_r(&paths, true).unwrap_err());
    let names: Vec<_> = missing.iter().map(|m| m.0.as_str()).collect();
    assert_eq!(names, vec!["ffmpeg", "model"]);
    assert_eq!(
        missing[0].1,
        format!(
            "configured path {} does not exist or is not executable",
            bad.display()
        )
    );
    assert_eq!(
        missing[1].1,
        format!("configured path {} is not a file", dir.path().display())
    );
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
    let out = run_r(&[bin], &RunOpts::default(), &mut |l| {
        lines.push(l.to_string())
    })
    .unwrap();
    assert!(out.status.success());
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
fn configured_path_beats_path_lookup() {
    let on_path = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let mut paths = fake_toolchain(on_path.path());
    let configured = script(other.path(), "my-ffmpeg", "echo 'ffmpeg version custom'");
    paths.ffmpeg = Some(configured.clone());
    let tc = preflight_r(&paths, true).unwrap();
    assert_eq!(tc.ffmpeg.path, configured);
    assert_eq!(tc.ffmpeg.version, "ffmpeg version custom");
}

/// Committed, already-executable fakes (`ffmpeg`, `ffprobe`, `whisper-cli`,
/// `spawn-grandchild`).
const FIXTURE_TOOLS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tools");
/// Set in the child process that [`run_child_test`] starts.
const CHILD_ENV: &str = "MINUTA_TOOLS_TEST_CHILD";

fn fixture_tool(name: &str) -> PathBuf {
    Path::new(FIXTURE_TOOLS).join(name)
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

#[test]
fn configured_missing_model_reported_without_need_model() {
    let dir = tempfile::tempdir().unwrap();
    let mut paths = fake_toolchain(dir.path());
    let typo = dir.path().join("ggml-bsae.bin");
    paths.model = Some(typo.clone());
    let missing = missing_of(preflight_r(&paths, false).unwrap_err());
    assert_eq!(
        missing,
        vec![(
            "model".to_string(),
            format!("configured path {} is not a file", typo.display())
        )]
    );
}

#[test]
fn inspect_reports_model_metadata_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut paths = fake_toolchain(dir.path());
    let file = dir.path().join("plain");
    fs::write(&file, "not a directory").unwrap();
    // A path under a regular file: metadata fails with ENOTDIR, not NotFound.
    let model = file.join("ggml-base.bin");
    paths.model = Some(model.clone());
    let report = inspect_r(&paths, false);
    assert_eq!(report.model, None);
    let missing: Vec<_> = report
        .missing
        .iter()
        .map(|m| (m.name.as_str(), m.hint.as_str()))
        .collect();
    let hint = format!(
        "cannot read configured path {}: Not a directory (os error 20)",
        model.display()
    );
    assert_eq!(missing, vec![("model", hint.as_str())]);
}

/// Runs the `#[ignore]`d test `name` of this binary in a child process whose `PATH` is
/// only the fixture tools dir. Setting `PATH` in this process would race with the other
/// test threads, which spawn tools.
fn run_child_test(name: &str) {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--ignored", "--test-threads=1"])
        .env_clear()
        .env("PATH", FIXTURE_TOOLS)
        .env(CHILD_ENV, "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("test result: ok. 1 passed"),
        "child test {name} failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn fixture_info(name: &str, version: &str) -> ToolInfo {
    ToolInfo {
        path: fixture_tool(name),
        version: version.into(),
    }
}

#[test]
fn inspect_falls_back_to_path_env() {
    run_child_test("child_inspect_uses_path_env");
}

#[test]
#[ignore = "run by inspect_falls_back_to_path_env in a child process"]
fn child_inspect_uses_path_env() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }
    let report = inspect(&ToolPaths::default(), false);
    assert_eq!(report.missing, vec![]);
    assert_eq!(
        report.ffmpeg,
        Some(fixture_info("ffmpeg", "ffmpeg version 0.0-fake"))
    );
    assert_eq!(
        report.ffprobe,
        Some(fixture_info("ffprobe", "ffprobe version 0.0-fake"))
    );
    assert_eq!(
        report.whisper,
        Some(fixture_info("whisper-cli", "whisper-cli 0.0-fake"))
    );
}

#[test]
fn locate_tool_falls_back_to_path_env() {
    run_child_test("child_locate_tool_uses_path_env");
}

#[test]
#[ignore = "run by locate_tool_falls_back_to_path_env in a child process"]
fn child_locate_tool_uses_path_env() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }
    let info = locate_tool("ffmpeg", "ffmpeg", "-version", "brew install ffmpeg").unwrap();
    assert_eq!(info, fixture_info("ffmpeg", "ffmpeg version 0.0-fake"));
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
        fixture_tool("spawn-grandchild").into_os_string(),
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
