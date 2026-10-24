use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use minuta::error::Error;
use minuta::job::{Ctx, Step};
use minuta::steps::probe::{read_probe, Probe, SourceProbe};
use minuta::tools::{inspect, run, RunOpts, ToolInfo, ToolPaths};

struct Tools {
    ffmpeg: ToolInfo,
    ffprobe: ToolInfo,
}

/// ffmpeg and ffprobe from `PATH`, or `None` (with a note) where they are not installed.
fn ffmpeg_available() -> Option<Tools> {
    let report = inspect(&ToolPaths::default(), false);
    match (report.ffmpeg, report.ffprobe) {
        (Some(ffmpeg), Some(ffprobe)) => Some(Tools { ffmpeg, ffprobe }),
        _ => {
            eprintln!("skipping: ffmpeg and ffprobe are not on PATH");
            None
        }
    }
}

fn argv(tool: &ToolInfo, args: &[&str]) -> Vec<OsString> {
    let mut v = vec![tool.path.clone().into_os_string()];
    v.extend(args.iter().map(OsString::from));
    v
}

fn ffmpeg(tools: &Tools, args: &[&str]) {
    let mut full = vec!["-hide_banner", "-nostdin", "-y"];
    full.extend_from_slice(args);
    run(&argv(&tools.ffmpeg, &full), RunOpts::default(), &mut |_| {}).unwrap();
}

/// A tiny mpeg4 + aac clip of a solid `color`, `size` (`WxH`) and `secs` seconds.
fn clip(tools: &Tools, dir: &Path, name: &str, color: &str, size: &str, secs: u32) -> PathBuf {
    let path = dir.join(name);
    let video = format!("color=c={color}:s={size}:r=25:d={secs}");
    let audio = format!("sine=frequency=440:d={secs}");
    ffmpeg(
        tools,
        &[
            "-f",
            "lavfi",
            "-i",
            &video,
            "-f",
            "lavfi",
            "-i",
            &audio,
            "-c:v",
            "mpeg4",
            "-c:a",
            "aac",
            "-shortest",
            path.to_str().unwrap(),
        ],
    );
    path
}

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

fn ctx(dir: &Path, sources: &[PathBuf]) -> Ctx {
    Ctx {
        dir: dir.to_path_buf(),
        sources: sources
            .iter()
            .map(|s| fs::canonicalize(s).unwrap())
            .collect(),
        source_digest: "digest".into(),
    }
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

    assert_eq!(step.outputs(&ctx), vec![tmp.path().join("probe.json")]);
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
    assert!(!tmp.path().join("probe.json").exists());
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
