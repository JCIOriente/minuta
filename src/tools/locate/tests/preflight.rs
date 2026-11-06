//! Finding the tools: `inspect`, `preflight` and `locate_tool` against scripted fakes, the
//! configured paths and the `PATH` environment variable.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::test_support::{fixture, retry_busy};

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// [`inspect`] under [`retry_busy`]. A report with missing items is handed to it as their
/// `Error::MissingTools`, so a "Text file busy" hint is retried like in [`preflight`]; the
/// last report is returned either way.
fn inspect_r(paths: &ToolPaths, need_model: bool) -> Report {
    let mut last = None;
    // Only the retries matter: the outcome is the report kept in `last`.
    let _ = retry_busy(|| {
        let report = inspect(paths, need_model);
        let missing = report.missing.clone();
        last = Some(report);
        if missing.is_empty() {
            Ok(())
        } else {
            Err(Error::MissingTools(missing))
        }
    });
    last.expect("retry_busy runs inspect at least once")
}

fn preflight_r(paths: &ToolPaths, need_model: bool) -> Result<Toolchain> {
    retry_busy(|| preflight(paths, need_model))
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

/// Set in the child process that [`run_child_test`] starts.
const CHILD_ENV: &str = "MINUTA_TOOLS_TEST_CHILD";

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
/// only the committed, already-executable fakes in `tests/fixtures/tools`. Setting `PATH` in this process would race with the other
/// test threads, which spawn tools.
fn run_child_test(name: &str) {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--ignored", "--test-threads=1"])
        .env_clear()
        .env("PATH", fixture("tools"))
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
        path: fixture("tools").join(name),
        version: version.into(),
    }
}

#[test]
fn inspect_falls_back_to_path_env() {
    run_child_test("tools::locate::tests::preflight::child_inspect_uses_path_env");
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
    run_child_test("tools::locate::tests::preflight::child_locate_tool_uses_path_env");
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
