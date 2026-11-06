use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::Error;
use crate::job::{write_atomic, Ctx, Step};
use crate::steps::join::{Encoder, Join};
use crate::steps::probe::SourceProbe;
use crate::steps::test_support::ctx;
#[cfg(unix)]
use crate::steps::test_support::{as_strings, fake_tool, lines_of, FAKE_STATS};
use crate::test_support::fixture;
use crate::tools::{RunOpts, ToolInfo};

#[test]
fn join_empty_sources_errors() {
    let tmp = tempfile::tempdir().unwrap();
    // A matching (empty) probe.json, so only the empty source list can fail.
    write_atomic(&crate::job::layout::probe_file(tmp.path()), |t| {
        Ok(fs::write(t, "[]")?)
    })
    .unwrap();
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
    let fewer = Ctx {
        sources: vec!["/a.mp4".into()],
        ..one.clone()
    };
    assert_ne!(base.params(&one), base.params(&fewer));
}

#[cfg(unix)]
#[test]
fn interrupt_stops_join_and_leaves_no_output() {
    let tmp = tempfile::tempdir().unwrap();
    // A committed fake that only sleeps, so no test thread writes an executable.
    let tool = ToolInfo {
        path: fixture("bin/hang"),
        version: "fake".into(),
    };
    let probe = |name: &str| SourceProbe {
        path: tmp.path().join(name),
        duration: 1.0,
        video: None,
        audio: crate::steps::probe::AudioStream {
            codec: "aac".into(),
            sample_rate: 48000,
            channels: 2,
        },
    };
    let sources = vec![probe("a.m4a"), probe("b.m4a")];
    write_atomic(&crate::job::layout::probe_file(tmp.path()), |t| {
        Ok(fs::write(t, serde_json::to_string(&sources).unwrap())?)
    })
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
    let opts = RunOpts {
        interrupt: Some(Arc::clone(&flag)),
        ..RunOpts::default()
    };
    let res = Join::new(tool, Encoder::X264).run_with(&ctx, opts, &mut |_| {});
    setter.join().unwrap();

    assert!(matches!(res, Err(Error::Interrupted)), "{res:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(!tmp.path().join("assets/joined.m4a").exists());
    assert!(!tmp.path().join("assets/joined.m4a.tmp").exists());
}

/// A job folder whose `probe.json` lists two audio-only sources at the given sample rates.
#[cfg(unix)]
fn audio_job(dir: &Path, rates: [u32; 2]) -> Ctx {
    let sources: Vec<SourceProbe> = ["a.m4a", "b.m4a"]
        .iter()
        .zip(rates)
        .map(|(name, rate)| SourceProbe {
            path: dir.join(name),
            duration: 1.0,
            video: None,
            audio: crate::steps::probe::AudioStream {
                codec: "aac".into(),
                sample_rate: rate,
                channels: 2,
            },
        })
        .collect();
    write_atomic(&crate::job::layout::probe_file(dir), |t| {
        Ok(fs::write(t, serde_json::to_string(&sources).unwrap())?)
    })
    .unwrap();
    Ctx {
        dir: dir.to_path_buf(),
        sources: sources.iter().map(|s| s.path.clone()).collect(),
        source_digest: "d".into(),
    }
}

#[cfg(unix)]
#[test]
fn join_stream_copy_argv_asks_for_stats_and_forwards_updates() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = audio_job(tmp.path(), [48000, 48000]);
    let mut lines = Vec::new();
    Join::new(fake_tool("ffmpeg"), Encoder::X264)
        .run_with(&ctx, RunOpts::default(), &mut |l| lines.push(l.to_string()))
        .unwrap();

    let list = tmp.path().join(".minuta/concat.txt");
    let tmp_out = tmp.path().join("assets/joined.m4a.tmp");
    let mut expected: Vec<String> = [
        "-hide_banner",
        "-nostdin",
        "-y",
        "-stats",
        "-f",
        "concat",
        "-safe",
        "0",
        "-i",
    ]
    .map(String::from)
    .to_vec();
    expected.extend(as_strings(&[&list]));
    expected.extend(["-c", "copy", "-f", "ipod"].map(String::from));
    expected.extend(as_strings(&[&tmp_out]));
    assert_eq!(lines_of(&tmp.path().join("assets/joined.m4a")), expected);
    assert_eq!(lines, FAKE_STATS);
}

#[cfg(unix)]
#[test]
fn join_reencode_argv_asks_for_stats() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = audio_job(tmp.path(), [48000, 44100]);
    Join::new(fake_tool("ffmpeg"), Encoder::X264)
        .run_with(&ctx, RunOpts::default(), &mut |_| {})
        .unwrap();

    let tmp_out = tmp.path().join("assets/joined.m4a.tmp");
    let mut expected: Vec<String> = ["-hide_banner", "-nostdin", "-y", "-stats", "-i"]
        .map(String::from)
        .to_vec();
    expected.extend(as_strings(&[&ctx.sources[0]]));
    expected.push("-i".into());
    expected.extend(as_strings(&[&ctx.sources[1]]));
    expected.extend(
        [
            "-filter_complex",
            "[0:a]aresample=48000,aformat=sample_fmts=fltp:sample_rates=48000:\
             channel_layouts=stereo[a0];\
             [1:a]aresample=48000,aformat=sample_fmts=fltp:sample_rates=48000:\
             channel_layouts=stereo[a1];\
             [a0][a1]concat=n=2:v=0:a=1[a]",
            "-map",
            "[a]",
            "-c:a",
            "aac",
            "-f",
            "ipod",
        ]
        .map(String::from),
    );
    expected.extend(as_strings(&[&tmp_out]));
    assert_eq!(lines_of(&tmp.path().join("assets/joined.m4a")), expected);
}
