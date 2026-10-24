#![cfg(unix)]

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use minuta::error::{Error, Result};
use minuta::tools::{inspect, preflight, run, Output, Report, RunOpts, ToolPaths, Toolchain};

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
        r"printf 'bad \377\376 bytes\r\n'; echo after1; echo after2 >&2; echo after3",
    )
    .into_os_string();
    let mut lines = Vec::new();
    let out = run_r(&[bin], &RunOpts::default(), &mut |l| {
        lines.push(l.to_string())
    })
    .unwrap();
    assert!(out.status.success());
    assert_eq!(lines[0], "bad \u{fffd}\u{fffd} bytes");
    lines.sort();
    assert!(lines.contains(&"after1".to_string()));
    assert!(lines.contains(&"after2".to_string()));
    assert!(lines.contains(&"after3".to_string()));
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
