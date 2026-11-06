use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::error::Error;
use crate::job::{Ctx, Step};
use crate::steps::probe::{read_probe, Probe, SourceProbe};
use crate::steps::test_support::{clip, ctx, ffmpeg, ffmpeg_available, Tools};
use crate::tools::ToolInfo;

/// A clip with a video stream and no audio stream.
fn silent_clip(tools: &Tools, dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    ffmpeg(
        tools,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=s=160x120:r=25:d=1",
            "-c:v",
            "mpeg4",
            path.to_str().unwrap(),
        ],
    );
    path
}

#[test]
fn probe_reads_streams() {
    let Some(t) = ffmpeg_available() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let a = clip(&t, tmp.path(), "a.mp4", "red", "160x120", 1);
    let b = clip(&t, tmp.path(), "b.mp4", "blue", "320x240", 2);
    let ctx = ctx(tmp.path(), &[a, b]);

    let step = Probe::new(t.ffprobe.clone());
    step.run(&ctx).unwrap();

    assert_eq!(
        step.outputs(&ctx),
        vec![crate::job::layout::probe_file(tmp.path())]
    );
    let probes: Vec<SourceProbe> = read_probe(tmp.path()).unwrap();
    assert_eq!(probes.len(), 2);
    assert_eq!(probes[0].path, ctx.sources[0]);
    let video = probes[0].video.as_ref().unwrap();
    assert_eq!(video.codec, "mpeg4");
    assert_eq!((video.width, video.height), (160, 120));
    assert_eq!(video.fps, "25/1");
    assert!(
        (probes[0].duration - 1.0).abs() < 0.2,
        "{}",
        probes[0].duration
    );
    assert_eq!(probes[0].audio.codec, "aac");
    let video = probes[1].video.as_ref().unwrap();
    assert_eq!((video.width, video.height), (320, 240));
    assert!(
        (probes[1].duration - 2.0).abs() < 0.2,
        "{}",
        probes[1].duration
    );
}

#[test]
fn no_audio_rejected() {
    let Some(t) = ffmpeg_available() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let a = clip(&t, tmp.path(), "a.mp4", "red", "160x120", 1);
    let quiet = silent_clip(&t, tmp.path(), "quiet.mp4");
    let ctx = ctx(tmp.path(), &[a, quiet.clone()]);

    let err = Probe::new(t.ffprobe.clone()).run(&ctx).unwrap_err();

    let expected = format!(
        "no audio stream in {}",
        fs::canonicalize(quiet).unwrap().display()
    );
    assert!(matches!(&err, Error::Job(m) if *m == expected), "{err}");
    assert!(!crate::job::layout::probe_file(tmp.path()).exists());
}

#[test]
fn probe_params_change_with_tool_version() {
    let Some(t) = ffmpeg_available() else { return };
    let ctx = ctx(Path::new("."), &[]);
    let mut newer = t.ffprobe.clone();
    newer.version.push_str(" (upgraded)");
    assert_ne!(
        Probe::new(t.ffprobe).params(&ctx),
        Probe::new(newer).params(&ctx)
    );
}

/// Runs `probe_reads_streams` in a child process whose `PATH` is an empty dir. Setting
/// `PATH` in this process would race with the other test threads, which spawn tools.
fn probe_test_without_ffmpeg(required: bool) -> Output {
    let empty = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "--exact",
            "steps::probe::tests::probe_reads_streams",
            "--nocapture",
        ])
        .env_clear()
        .env("PATH", empty.path());
    if required {
        child.env("MINUTA_REQUIRE_FFMPEG", "1");
    }
    child.output().unwrap()
}

#[test]
fn missing_ffmpeg_fails_when_required() {
    let out = probe_test_without_ffmpeg(true);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{stderr}");
    assert!(
        stderr
            .lines()
            .any(|l| l == "MINUTA_REQUIRE_FFMPEG=1 but ffmpeg and ffprobe are not on PATH"),
        "{stderr}"
    );
}

#[test]
fn missing_ffmpeg_skips_when_not_required() {
    let out = probe_test_without_ffmpeg(false);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(
        stderr
            .lines()
            .any(|l| l == "skipping: ffmpeg and ffprobe are not on PATH"),
        "{stderr}"
    );
}

#[test]
fn probe_params_change_with_sources() {
    let tool = ToolInfo {
        path: "ffprobe".into(),
        version: "ffprobe version 1".into(),
    };
    let one = Ctx {
        dir: ".".into(),
        sources: vec!["/a.mp4".into(), "/b.mp4".into()],
        source_digest: "d".into(),
    };
    let moved = Ctx {
        sources: vec!["/moved/a.mp4".into(), "/moved/b.mp4".into()],
        ..one.clone()
    };
    let step = Probe::new(tool);
    assert_ne!(step.params(&one), step.params(&moved));
}
