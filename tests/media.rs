use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use minuta::error::Error;
use minuta::job::{Ctx, Step};
use minuta::steps::audio::Audio;
use minuta::steps::join::{joined_path, plan_join, Encoder, Join, Mode};
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

// ---- join ----

fn probe_all(t: &Tools, dir: &Path, sources: &[PathBuf]) -> Ctx {
    let ctx = ctx(dir, sources);
    Probe::new(t.ffprobe.clone()).run(&ctx).unwrap();
    ctx
}

/// Probes one finished file, using a scratch job folder of its own.
fn describe(t: &Tools, file: &Path) -> SourceProbe {
    let scratch = tempfile::tempdir().unwrap();
    probe_all(t, scratch.path(), &[file.to_path_buf()]);
    read_probe(scratch.path()).unwrap().remove(0)
}

/// The mean colour of one frame of `file`, taken from its start or from near its end.
fn colour(t: &Tools, file: &Path, from_end: bool) -> [u8; 3] {
    let scratch = tempfile::tempdir().unwrap();
    let raw = scratch.path().join("frame.rgb");
    let mut args = vec!["-an"];
    if from_end {
        args.extend(["-sseof", "-0.3"]);
    }
    args.extend([
        "-i",
        file.to_str().unwrap(),
        "-vf",
        "scale=1:1",
        "-frames:v",
        "1",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgb24",
        raw.to_str().unwrap(),
    ]);
    ffmpeg(t, &args);
    let bytes = fs::read(raw).unwrap();
    [bytes[0], bytes[1], bytes[2]]
}

fn is_red(c: [u8; 3]) -> bool {
    c[0] > c[2].saturating_add(100)
}

fn is_blue(c: [u8; 3]) -> bool {
    c[2] > c[0].saturating_add(100)
}

#[test]
fn single_source_links() {
    use std::os::unix::fs::MetadataExt;
    let Some(t) = ffmpeg_available() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let a = clip(&t, tmp.path(), "a.mp4", "red", "160x120", 1);
    let ctx = probe_all(&t, tmp.path(), std::slice::from_ref(&a));

    let step = Join::new(t.ffmpeg.clone(), Encoder::X264);
    step.run(&ctx).unwrap();

    let joined = tmp.path().join("joined.mp4");
    assert_eq!(step.outputs(&ctx), vec![joined.clone()]);
    assert_eq!(joined_path(&ctx), joined);
    let (src, dst) = (fs::metadata(&a).unwrap(), fs::metadata(&joined).unwrap());
    assert_eq!(
        (src.dev(), src.ino()),
        (dst.dev(), dst.ino()),
        "not a hard link"
    );
    assert!(!tmp.path().join("joined.mp4.tmp").exists());
}

#[test]
fn matching_sources_stream_copy() {
    let Some(t) = ffmpeg_available() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let a = clip(&t, tmp.path(), "a.mp4", "red", "160x120", 1);
    let b = clip(&t, tmp.path(), "b.mp4", "blue", "160x120", 1);
    let ctx = probe_all(&t, tmp.path(), &[a, b]);
    assert_eq!(
        plan_join(&read_probe(tmp.path()).unwrap()),
        Mode::StreamCopy
    );

    Join::new(t.ffmpeg.clone(), Encoder::X264)
        .run(&ctx)
        .unwrap();

    // Re-encoding would turn mpeg4 into h264, so mpeg4 proves the streams were copied.
    let joined = describe(&t, &tmp.path().join("joined.mp4"));
    assert_eq!(joined.video.unwrap().codec, "mpeg4");
    assert!((joined.duration - 2.0).abs() < 0.3, "{}", joined.duration);
    assert!(!tmp.path().join("joined.mp4.tmp").exists());
}

#[test]
fn mismatched_sources_reencode() {
    let Some(t) = ffmpeg_available() else { return };
    let listing = encoders(&t);
    for encoder in [Encoder::X264, Encoder::VideoToolbox] {
        if !listing.contains(encoder.name()) {
            eprintln!("skipping {}: not built into this ffmpeg", encoder.name());
            continue;
        }
        let tmp = tempfile::tempdir().unwrap();
        let a = clip(&t, tmp.path(), "a.mp4", "red", "320x240", 1);
        let b = clip(&t, tmp.path(), "b.mp4", "blue", "640x480", 1);
        let ctx = probe_all(&t, tmp.path(), &[a, b]);
        assert_eq!(plan_join(&read_probe(tmp.path()).unwrap()), Mode::Reencode);

        Join::new(t.ffmpeg.clone(), encoder).run(&ctx).unwrap();

        let joined = describe(&t, &tmp.path().join("joined.mp4"));
        let video = joined.video.unwrap();
        assert_eq!(video.codec, "h264", "{}", encoder.name());
        assert_eq!((video.width, video.height), (320, 240));
        assert!((joined.duration - 2.0).abs() < 0.3, "{}", joined.duration);
    }
}

#[test]
fn join_preserves_order() {
    let Some(t) = ffmpeg_available() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let red = clip(&t, tmp.path(), "red.mp4", "red", "160x120", 1);
    let blue = clip(&t, tmp.path(), "blue.mp4", "blue", "160x120", 1);
    for (name, order, first_is_red) in [("one", [&red, &blue], true), ("two", [&blue, &red], false)]
    {
        let dir = tmp.path().join(name);
        fs::create_dir(&dir).unwrap();
        let ctx = probe_all(&t, &dir, &[order[0].clone(), order[1].clone()]);
        Join::new(t.ffmpeg.clone(), Encoder::X264)
            .run(&ctx)
            .unwrap();

        let joined = dir.join("joined.mp4");
        let (first, last) = (colour(&t, &joined, false), colour(&t, &joined, true));
        if first_is_red {
            assert!(is_red(first) && is_blue(last), "{first:?} {last:?}");
        } else {
            assert!(is_blue(first) && is_red(last), "{first:?} {last:?}");
        }
        let list = fs::read_to_string(dir.join("concat.txt")).unwrap();
        let names: Vec<_> = list.lines().collect();
        assert!(names[0].contains(order[0].file_name().unwrap().to_str().unwrap()));
    }
}

#[test]
fn reencode_preserves_order() {
    let Some(t) = ffmpeg_available() else { return };
    if !encoders(&t).contains("libx264") {
        eprintln!("skipping: this ffmpeg has no libx264");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    // Different sizes force a re-encode.
    let red = clip(&t, tmp.path(), "red.mp4", "red", "320x240", 1);
    let blue = clip(&t, tmp.path(), "blue.mp4", "blue", "640x480", 1);
    for (name, order, first_is_red) in [("one", [&red, &blue], true), ("two", [&blue, &red], false)]
    {
        let dir = tmp.path().join(name);
        fs::create_dir(&dir).unwrap();
        let ctx = probe_all(&t, &dir, &[order[0].clone(), order[1].clone()]);
        assert_eq!(plan_join(&read_probe(&dir).unwrap()), Mode::Reencode);

        Join::new(t.ffmpeg.clone(), Encoder::X264)
            .run(&ctx)
            .unwrap();

        let joined = dir.join("joined.mp4");
        let (first, last) = (colour(&t, &joined, false), colour(&t, &joined, true));
        if first_is_red {
            assert!(is_red(first) && is_blue(last), "{name}: {first:?} {last:?}");
        } else {
            assert!(is_blue(first) && is_red(last), "{name}: {first:?} {last:?}");
        }
    }
}

/// A one second 160x120 mpeg4 + aac clip of a solid red, with audio at `rate` Hz.
fn clip_at_rate(tools: &Tools, dir: &Path, name: &str, rate: u32) -> PathBuf {
    let path = dir.join(name);
    let audio = format!("sine=frequency=440:sample_rate={rate}:d=1");
    ffmpeg(
        tools,
        &[
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=160x120:r=25:d=1",
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

#[test]
fn differing_audio_forces_reencode() {
    let Some(t) = ffmpeg_available() else { return };
    if !encoders(&t).contains("libx264") {
        eprintln!("skipping: this ffmpeg has no libx264");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let a = clip_at_rate(&t, tmp.path(), "a.mp4", 44100);
    let b = clip_at_rate(&t, tmp.path(), "b.mp4", 22050);
    let ctx = probe_all(&t, tmp.path(), &[a, b]);
    let probes = read_probe(tmp.path()).unwrap();
    // Same video parameters; only the audio differs.
    assert_eq!(probes[0].video, probes[1].video);
    assert_eq!(
        (probes[0].audio.sample_rate, probes[1].audio.sample_rate),
        (44100, 22050)
    );
    assert_eq!(plan_join(&probes), Mode::Reencode);

    Join::new(t.ffmpeg.clone(), Encoder::X264)
        .run(&ctx)
        .unwrap();

    // Stream copy would keep mpeg4 and 44100 Hz; the re-encode gives h264 at 48000 Hz.
    let joined = describe(&t, &tmp.path().join("joined.mp4"));
    assert_eq!(joined.video.unwrap().codec, "h264");
    assert_eq!(
        (joined.audio.codec.as_str(), joined.audio.sample_rate),
        ("aac", 48000)
    );
    assert!((joined.duration - 2.0).abs() < 0.3, "{}", joined.duration);
}

#[test]
fn join_empty_sources_errors() {
    let tmp = tempfile::tempdir().unwrap();
    // A matching (empty) probe.json, so only the empty source list can fail.
    fs::write(tmp.path().join("probe.json"), "[]").unwrap();
    let ctx = ctx(tmp.path(), &[]);
    let tool = ToolInfo {
        path: "ffmpeg".into(),
        version: "ffmpeg version 1".into(),
    };

    let err = Join::new(tool, Encoder::X264).run(&ctx).unwrap_err();

    assert!(
        matches!(&err, Error::Job(m) if m == "no sources to join"),
        "{err}"
    );
    assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
}

fn encoders(t: &Tools) -> String {
    let mut out = String::new();
    run(
        &argv(&t.ffmpeg, &["-hide_banner", "-encoders"]),
        RunOpts::default(),
        &mut |l| {
            out.push_str(l);
            out.push('\n');
        },
    )
    .unwrap();
    out
}

#[test]
fn encoder_choice_falls_back_to_x264() {
    let with = " V....D h264_videotoolbox    VideoToolbox H.264 Encoder (codec h264)\n \
                V....D libx264              libx264 H.264\n";
    let without = " V....D libx264              libx264 H.264\n V.S..D mpeg4   MPEG-4 part 2\n";
    assert_eq!(Encoder::pick(with), Encoder::VideoToolbox);
    assert_eq!(Encoder::pick(without), Encoder::X264);
    assert_eq!(Encoder::pick(""), Encoder::X264);
}

#[test]
fn join_params_change_with_inputs() {
    let tool = ToolInfo {
        path: "ffmpeg".into(),
        version: "ffmpeg version 1".into(),
    };
    let one = Ctx {
        dir: ".".into(),
        sources: vec!["/a.mp4".into(), "/b.mp4".into()],
        source_digest: "d".into(),
    };
    let reordered = Ctx {
        sources: vec!["/b.mp4".into(), "/a.mp4".into()],
        ..one.clone()
    };
    let base = Join::new(tool.clone(), Encoder::X264);
    let upgraded = ToolInfo {
        version: "ffmpeg version 2".into(),
        ..tool.clone()
    };
    assert_ne!(
        base.params(&one),
        Join::new(tool, Encoder::VideoToolbox).params(&one)
    );
    assert_ne!(
        base.params(&one),
        Join::new(upgraded, Encoder::X264).params(&one)
    );
    assert_ne!(base.params(&one), base.params(&reordered));
}

// ---- audio ----

#[test]
fn audio_is_mono_16k() {
    let Some(t) = ffmpeg_available() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let a = clip(&t, tmp.path(), "a.mp4", "red", "160x120", 2);
    let ctx = probe_all(&t, tmp.path(), &[a]);
    Join::new(t.ffmpeg.clone(), Encoder::X264)
        .run(&ctx)
        .unwrap();

    let step = Audio::new(t.ffmpeg.clone());
    step.run(&ctx).unwrap();

    let wav = tmp.path().join("audio.wav");
    assert_eq!(step.outputs(&ctx), vec![wav.clone()]);
    let probed = describe(&t, &wav);
    assert!(probed.video.is_none());
    assert_eq!(probed.audio.codec, "pcm_s16le");
    assert_eq!(
        (probed.audio.channels, probed.audio.sample_rate),
        (1, 16000)
    );
    assert!((probed.duration - 2.0).abs() < 0.3, "{}", probed.duration);
    assert!(!tmp.path().join("audio.wav.tmp").exists());
}

#[test]
fn audio_params_change_with_tool_version() {
    let tool = ToolInfo {
        path: "ffmpeg".into(),
        version: "ffmpeg version 1".into(),
    };
    let newer = ToolInfo {
        version: "ffmpeg version 2".into(),
        ..tool.clone()
    };
    let ctx = ctx(Path::new("."), &[]);
    assert_ne!(
        Audio::new(tool).params(&ctx),
        Audio::new(newer).params(&ctx)
    );
}

// ---- audio-only, containers, interrupt ----

/// A one second sine WAV at `rate` Hz.
fn tone(t: &Tools, dir: &Path, name: &str, rate: u32) -> PathBuf {
    let path = dir.join(name);
    let src = format!("sine=frequency=440:sample_rate={rate}:d=1");
    ffmpeg(t, &["-f", "lavfi", "-i", &src, path.to_str().unwrap()]);
    path
}

#[test]
fn audio_only_sources_reencode_to_wav() {
    let Some(t) = ffmpeg_available() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let a = tone(&t, tmp.path(), "a.wav", 8000);
    let b = tone(&t, tmp.path(), "b.wav", 16000);
    let ctx = probe_all(&t, tmp.path(), &[a, b]);
    assert_eq!(plan_join(&read_probe(tmp.path()).unwrap()), Mode::Reencode);

    Join::new(t.ffmpeg.clone(), Encoder::X264)
        .run(&ctx)
        .unwrap();

    let joined = describe(&t, &tmp.path().join("joined.wav"));
    assert!(joined.video.is_none());
    assert_eq!(joined.audio.codec, "pcm_s16le");
    assert!((joined.duration - 2.0).abs() < 0.3, "{}", joined.duration);
}

#[test]
fn audio_only_and_video_sources_rejected() {
    let Some(t) = ffmpeg_available() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let a = clip(&t, tmp.path(), "a.mp4", "red", "160x120", 1);
    let b = tone(&t, tmp.path(), "b.wav", 16000);
    let ctx = probe_all(&t, tmp.path(), &[a, b]);

    let err = Join::new(t.ffmpeg.clone(), Encoder::X264)
        .run(&ctx)
        .unwrap_err();

    assert!(
        matches!(&err, Error::Job(m) if m == "cannot join audio-only and video sources"),
        "{err}"
    );
    assert!(!tmp.path().join("joined.mkv").exists());
}

#[test]
fn webm_sources_keep_their_container() {
    let Some(t) = ffmpeg_available() else { return };
    let listing = encoders(&t);
    if !listing.contains("libvpx ") || !listing.contains("libopus") {
        eprintln!("skipping: this ffmpeg has no libvpx or libopus");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let mut sources = Vec::new();
    for name in ["a.webm", "b.webm"] {
        let path = tmp.path().join(name);
        ffmpeg(
            &t,
            &[
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=160x120:r=25:d=1",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:d=1",
                "-c:v",
                "libvpx",
                "-c:a",
                "libopus",
                "-shortest",
                path.to_str().unwrap(),
            ],
        );
        sources.push(path);
    }
    let ctx = probe_all(&t, tmp.path(), &sources);

    Join::new(t.ffmpeg.clone(), Encoder::X264)
        .run(&ctx)
        .unwrap();

    let joined = describe(&t, &tmp.path().join("joined.webm"));
    assert_eq!(joined.video.unwrap().codec, "vp8");
    assert_eq!(joined.audio.codec, "opus");
    assert!((joined.duration - 2.0).abs() < 0.3, "{}", joined.duration);
}

/// Retry while a freshly written script is still held open by a fork in another test
/// thread (ETXTBSY on Linux).
fn retry<T>(mut f: impl FnMut() -> minuta::error::Result<T>) -> minuta::error::Result<T> {
    let mut attempts = 0;
    loop {
        let res = f();
        attempts += 1;
        let busy = matches!(&res, Err(Error::Io(e)) if e.to_string().contains("Text file busy"));
        if !busy || attempts >= 20 {
            return res;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(unix)]
#[test]
fn interrupt_stops_join_and_leaves_no_output() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let fake = tmp.path().join("ffmpeg");
    fs::write(&fake, "#!/bin/sh\nexec sleep 30\n").unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    let tool = ToolInfo {
        path: fake,
        version: "fake".into(),
    };
    let probe = |name: &str| SourceProbe {
        path: tmp.path().join(name),
        duration: 1.0,
        video: None,
        audio: minuta::steps::probe::AudioStream {
            codec: "aac".into(),
            sample_rate: 48000,
            channels: 2,
        },
    };
    let sources = vec![probe("a.m4a"), probe("b.m4a")];
    fs::write(
        tmp.path().join("probe.json"),
        serde_json::to_string(&sources).unwrap(),
    )
    .unwrap();
    let ctx = Ctx {
        dir: tmp.path().to_path_buf(),
        sources: sources.iter().map(|s| s.path.clone()).collect(),
        source_digest: "d".into(),
    };
    let flag = Arc::new(AtomicBool::new(false));
    let setter = {
        let flag = Arc::clone(&flag);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            flag.store(true, Ordering::SeqCst);
        })
    };

    let started = Instant::now();
    let res = retry(|| {
        let opts = RunOpts {
            interrupt: Some(Arc::clone(&flag)),
            ..RunOpts::default()
        };
        Join::new(tool.clone(), Encoder::X264).run_with(&ctx, opts, &mut |_| {})
    });
    setter.join().unwrap();

    assert!(matches!(res, Err(Error::Interrupted)), "{res:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(!tmp.path().join("joined.m4a").exists());
    assert!(!tmp.path().join("joined.m4a.tmp").exists());
}

#[test]
fn run_with_forwards_tool_output() {
    let Some(t) = ffmpeg_available() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let a = clip(&t, tmp.path(), "a.mp4", "red", "160x120", 1);
    let ctx = ctx(tmp.path(), &[a]);

    let mut probe_lines = 0;
    Probe::new(t.ffprobe.clone())
        .run_with(&ctx, RunOpts::default(), &mut |_| probe_lines += 1)
        .unwrap();
    Join::new(t.ffmpeg.clone(), Encoder::X264)
        .run(&ctx)
        .unwrap();
    let mut audio_lines = 0;
    Audio::new(t.ffmpeg.clone())
        .run_with(&ctx, RunOpts::default(), &mut |_| audio_lines += 1)
        .unwrap();

    assert!(probe_lines > 0 && audio_lines > 0);
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

#[test]
fn reencode_reads_moved_sources() {
    let Some(t) = ffmpeg_available() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let old = tmp.path().join("old");
    let new = tmp.path().join("new");
    fs::create_dir(&old).unwrap();
    fs::create_dir(&new).unwrap();
    let a = clip(&t, &old, "a.mp4", "red", "320x240", 1);
    let b = clip(&t, &old, "b.mp4", "blue", "640x480", 1);
    let job = tmp.path().join("job");
    fs::create_dir(&job).unwrap();
    probe_all(&t, &job, &[a.clone(), b.clone()]);
    // The recordings move; probe.json still names the old paths.
    let moved: Vec<PathBuf> = [&a, &b]
        .iter()
        .map(|s| {
            let to = new.join(s.file_name().unwrap());
            fs::rename(s, &to).unwrap();
            to
        })
        .collect();

    Join::new(t.ffmpeg.clone(), Encoder::X264)
        .run(&ctx(&job, &moved))
        .unwrap();

    let joined = describe(&t, &job.join("joined.mp4"));
    assert!((joined.duration - 2.0).abs() < 0.3, "{}", joined.duration);

    let err = Join::new(t.ffmpeg.clone(), Encoder::X264)
        .run(&ctx(&job, &moved[..1]))
        .unwrap_err();
    assert!(matches!(err, Error::Job(_)), "{err}");
}
