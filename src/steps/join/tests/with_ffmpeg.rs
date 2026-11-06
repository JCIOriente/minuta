use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::job::Step;
use crate::steps::join::{joined_path, plan_join, Encoder, Join, Mode};
use crate::steps::probe::read_probe;
use crate::steps::test_support::{
    argv, clip, ctx, describe, ffmpeg, ffmpeg_available, probe_all, tree, Tools,
};
use crate::tools::{run, RunOpts};

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
fn single_source_has_no_joined_file() {
    let Some(t) = ffmpeg_available() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let job = tmp.path().join("job");
    fs::create_dir(&job).unwrap();
    let a = clip(&t, tmp.path(), "a.mp4", "red", "160x120", 1);
    let ctx = probe_all(&t, &job, std::slice::from_ref(&a));

    let step = Join::new(t.ffmpeg.clone(), Encoder::X264);
    step.run(&ctx).unwrap();

    assert_eq!(step.outputs(&ctx), Vec::<PathBuf>::new());
    assert_eq!(joined_path(&ctx), None);
    assert!(
        tree(&job).iter().all(|f| !f.contains("joined")),
        "{:?}",
        tree(&job)
    );
}

#[test]
fn several_sources_join_into_assets() {
    let Some(t) = ffmpeg_available() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let job = tmp.path().join("job");
    fs::create_dir(&job).unwrap();
    let a = clip(&t, tmp.path(), "a.mp4", "red", "160x120", 1);
    let b = clip(&t, tmp.path(), "b.mp4", "blue", "160x120", 1);
    let ctx = probe_all(&t, &job, &[a, b]);

    let step = Join::new(t.ffmpeg.clone(), Encoder::X264);
    step.run(&ctx).unwrap();

    let joined = job.join("assets/joined.mp4");
    assert_eq!(step.outputs(&ctx), vec![joined.clone()]);
    assert_eq!(joined_path(&ctx), Some(joined));
    let files: Vec<String> = tree(&job)
        .into_iter()
        .filter(|f| f.contains("joined"))
        .collect();
    assert_eq!(files, ["assets/joined.mp4"]);
    assert_eq!(
        tree(&job),
        [".minuta/probe.json", "assets/joined.mp4"],
        "the concat list is removed after the join"
    );
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
        Some(Mode::StreamCopy)
    );

    Join::new(t.ffmpeg.clone(), Encoder::X264)
        .run(&ctx)
        .unwrap();

    // Re-encoding would turn mpeg4 into h264, so mpeg4 proves the streams were copied.
    let joined = describe(&t, &tmp.path().join("assets/joined.mp4"));
    assert_eq!(joined.video.unwrap().codec, "mpeg4");
    assert!((joined.duration - 2.0).abs() < 0.3, "{}", joined.duration);
    assert!(!tmp.path().join("assets/joined.mp4.tmp").exists());
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
        assert_eq!(
            plan_join(&read_probe(tmp.path()).unwrap()),
            Some(Mode::Reencode)
        );

        Join::new(t.ffmpeg.clone(), encoder).run(&ctx).unwrap();

        let joined = describe(&t, &tmp.path().join("assets/joined.mp4"));
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

        let joined = dir.join("assets/joined.mp4");
        let (first, last) = (colour(&t, &joined, false), colour(&t, &joined, true));
        if first_is_red {
            assert!(is_red(first) && is_blue(last), "{first:?} {last:?}");
        } else {
            assert!(is_blue(first) && is_red(last), "{first:?} {last:?}");
        }
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
        assert_eq!(plan_join(&read_probe(&dir).unwrap()), Some(Mode::Reencode));

        Join::new(t.ffmpeg.clone(), Encoder::X264)
            .run(&ctx)
            .unwrap();

        let joined = dir.join("assets/joined.mp4");
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
    assert_eq!(plan_join(&probes), Some(Mode::Reencode));

    Join::new(t.ffmpeg.clone(), Encoder::X264)
        .run(&ctx)
        .unwrap();

    // Stream copy would keep mpeg4 and 44100 Hz; the re-encode gives h264 at 48000 Hz.
    let joined = describe(&t, &tmp.path().join("assets/joined.mp4"));
    assert_eq!(joined.video.unwrap().codec, "h264");
    assert_eq!(
        (joined.audio.codec.as_str(), joined.audio.sample_rate),
        ("aac", 48000)
    );
    assert!((joined.duration - 2.0).abs() < 0.3, "{}", joined.duration);
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
    assert_eq!(
        plan_join(&read_probe(tmp.path()).unwrap()),
        Some(Mode::Reencode)
    );

    Join::new(t.ffmpeg.clone(), Encoder::X264)
        .run(&ctx)
        .unwrap();

    let joined = describe(&t, &tmp.path().join("assets/joined.wav"));
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
    assert!(!tmp.path().join("assets/joined.mkv").exists());
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

    let joined = describe(&t, &tmp.path().join("assets/joined.webm"));
    assert_eq!(joined.video.unwrap().codec, "vp8");
    assert_eq!(joined.audio.codec, "opus");
    assert!((joined.duration - 2.0).abs() < 0.3, "{}", joined.duration);
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

    let joined = describe(&t, &job.join("assets/joined.mp4"));
    assert!((joined.duration - 2.0).abs() < 0.3, "{}", joined.duration);

    let err = Join::new(t.ffmpeg.clone(), Encoder::X264)
        .run(&ctx(
            &job,
            &[moved[0].clone(), moved[0].clone(), moved[1].clone()],
        ))
        .unwrap_err();
    assert!(matches!(err, Error::Job(_)), "{err}");
}
