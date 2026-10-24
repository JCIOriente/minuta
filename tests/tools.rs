#![cfg(unix)]

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use minuta::error::Error;
use minuta::tools::{preflight, run, RunOpts, ToolPaths};

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn argv(parts: &[&OsString]) -> Vec<OsString> {
    parts.iter().map(|p| (*p).clone()).collect()
}

#[test]
fn argv_with_shell_metacharacters_is_safe() {
    let dir = tempfile::tempdir().unwrap();
    let echo = script(dir.path(), "echo1", r#"printf '%s\n' "$1""#);
    let name = OsString::from(r#"a b"$(touch pwned)'.mp4"#);
    let bin = echo.into_os_string();
    let opts = RunOpts {
        cwd: Some(dir.path().to_path_buf()),
        ..RunOpts::default()
    };
    let mut lines = Vec::new();
    run(&argv(&[&bin, &name]), opts, &mut |l| {
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
    let out = run(&[bin], RunOpts::default(), &mut |l| {
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
    let bin = script(dir.path(), "bad", "echo boom >&2\nexit 3").into_os_string();
    let err = run(&[bin], RunOpts::default(), &mut |_| {}).unwrap_err();
    match err {
        Error::Tool { code, stderr, .. } => {
            assert_eq!(code, Some(3));
            assert!(stderr.contains("boom"), "stderr was {stderr:?}");
        }
        other => panic!("expected Tool error, got {other:?}"),
    }
}

#[test]
fn interrupt_kills_child() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(dir.path(), "slow", "exec sleep 30").into_os_string();
    let flag = Arc::new(AtomicBool::new(false));
    let setter = Arc::clone(&flag);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        setter.store(true, Ordering::SeqCst);
    });
    let opts = RunOpts {
        interrupt: Some(flag),
        ..RunOpts::default()
    };
    let start = Instant::now();
    let err = run(&[bin], opts, &mut |_| {}).unwrap_err();
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
    match preflight(&paths, true).unwrap_err() {
        Error::MissingTools(missing) => {
            let names: Vec<_> = missing.iter().map(|m| m.name.as_str()).collect();
            assert_eq!(names, vec!["ffmpeg", "ffprobe", "whisper-cli", "model"]);
            assert!(missing.iter().all(|m| !m.hint.is_empty()));
        }
        other => panic!("expected MissingTools, got {other:?}"),
    }
    // The model is only required when asked for.
    match preflight(&paths, false).unwrap_err() {
        Error::MissingTools(missing) => assert_eq!(missing.len(), 3),
        other => panic!("expected MissingTools, got {other:?}"),
    }
}

#[test]
fn version_captured() {
    let dir = tempfile::tempdir().unwrap();
    script(
        dir.path(),
        "ffmpeg",
        "echo 'ffmpeg version 7.1 extra'\necho second",
    );
    script(dir.path(), "ffprobe", "echo 'ffprobe version 7.1'");
    let whisper = script(dir.path(), "whisper-cli", "echo 'whisper 1.7.2'");
    let model = dir.path().join("ggml-base.bin");
    fs::write(&model, b"x").unwrap();
    // ffmpeg and ffprobe come from PATH, whisper and the model from overrides.
    let paths = ToolPaths {
        path: Some(dir.path().as_os_str().to_owned()),
        whisper: Some(whisper),
        model: Some(model.clone()),
        ..ToolPaths::default()
    };
    let tc = preflight(&paths, true).unwrap();
    assert_eq!(tc.ffmpeg.version, "ffmpeg version 7.1 extra");
    assert_eq!(tc.ffprobe.version, "ffprobe version 7.1");
    assert_eq!(tc.whisper.version, "whisper 1.7.2");
    assert_eq!(tc.ffmpeg.path, dir.path().join("ffmpeg"));
    assert_eq!(tc.model, Some(model));
}

#[test]
fn interrupt_kills_child_that_closed_stdio() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(dir.path(), "quiet", "exec >/dev/null 2>&1\nexec sleep 30").into_os_string();
    let flag = Arc::new(AtomicBool::new(false));
    let setter = Arc::clone(&flag);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        setter.store(true, Ordering::SeqCst);
    });
    let opts = RunOpts {
        interrupt: Some(flag),
        ..RunOpts::default()
    };
    let start = Instant::now();
    let err = run(&[bin], opts, &mut |_| {}).unwrap_err();
    assert!(matches!(err, Error::Interrupted), "got {err:?}");
    assert!(start.elapsed() < Duration::from_secs(1));
    // The pipes closed long before the flag was set, so this proves the wait is interruptible.
}
