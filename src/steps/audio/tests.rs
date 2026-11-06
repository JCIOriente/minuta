use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::job::{Ctx, Step};
use crate::steps::audio::Audio;
use crate::steps::join::{Encoder, Join};
#[cfg(unix)]
use crate::steps::test_support::{as_strings, fake_tool, lines_of, FAKE_STATS};
use crate::steps::test_support::{clip, ctx, describe, ffmpeg_available, probe_all, tree};
use crate::tools::{RunOpts, ToolInfo};

#[test]
fn audio_is_extracted_from_the_original_for_one_source() {
    let Some(t) = ffmpeg_available() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let job = tmp.path().join("job");
    fs::create_dir(&job).unwrap();
    let a = clip(&t, tmp.path(), "a.mp4", "red", "160x120", 2);
    let ctx = probe_all(&t, &job, &[a]);

    // No join step ran: the audio comes straight from the source.
    let step = Audio::new(t.ffmpeg.clone());
    step.run(&ctx).unwrap();

    let wav = job.join("assets/audio.wav");
    assert_eq!(step.outputs(&ctx), vec![wav.clone()]);
    let probed = describe(&t, &wav);
    assert_eq!(
        (probed.audio.channels, probed.audio.sample_rate),
        (1, 16000)
    );
    assert!((probed.duration - 2.0).abs() < 0.3, "{}", probed.duration);
    assert_eq!(tree(&job), [".minuta/probe.json", "assets/audio.wav"]);
}

#[test]
fn audio_of_several_sources_comes_from_the_joined_file() {
    let Some(t) = ffmpeg_available() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let job = tmp.path().join("job");
    fs::create_dir(&job).unwrap();
    let a = clip(&t, tmp.path(), "a.mp4", "red", "160x120", 1);
    let b = clip(&t, tmp.path(), "b.mp4", "blue", "160x120", 1);
    let ctx = probe_all(&t, &job, &[a, b]);
    Join::new(t.ffmpeg.clone(), Encoder::X264)
        .run(&ctx)
        .unwrap();

    let step = Audio::new(t.ffmpeg.clone());
    step.run(&ctx).unwrap();

    let wav = job.join("assets/audio.wav");
    let probed = describe(&t, &wav);
    assert!(probed.video.is_none());
    assert_eq!(probed.audio.codec, "pcm_s16le");
    assert_eq!(
        (probed.audio.channels, probed.audio.sample_rate),
        (1, 16000)
    );
    assert!((probed.duration - 2.0).abs() < 0.3, "{}", probed.duration);
    assert!(!job.join("assets/audio.wav.tmp").exists());
}

#[test]
fn audio_without_sources_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let tool = ToolInfo {
        path: "ffmpeg".into(),
        version: "ffmpeg version 1".into(),
    };

    let err = Audio::new(tool).run(&ctx(tmp.path(), &[])).unwrap_err();

    assert!(
        matches!(&err, Error::Job(m) if m == "no sources to extract audio from"),
        "{err}"
    );
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

#[cfg(unix)]
#[test]
fn audio_ffmpeg_argv_asks_for_stats_and_forwards_updates() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = Ctx {
        dir: tmp.path().to_path_buf(),
        sources: vec!["/rec/a.mp4".into()],
        source_digest: "d".into(),
    };
    let mut lines = Vec::new();
    Audio::new(fake_tool("ffmpeg"))
        .run_with(&ctx, RunOpts::default(), &mut |l| lines.push(l.to_string()))
        .unwrap();

    let input = PathBuf::from("/rec/a.mp4");
    let tmp_out = tmp.path().join("assets/audio.wav.tmp");
    let mut expected: Vec<String> = ["-hide_banner", "-nostdin", "-y", "-stats", "-i"]
        .map(String::from)
        .to_vec();
    expected.extend(as_strings(&[&input]));
    expected.extend(
        [
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "pcm_s16le",
            "-f",
            "wav",
        ]
        .map(String::from),
    );
    expected.extend(as_strings(&[&tmp_out]));
    assert_eq!(lines_of(&tmp.path().join("assets/audio.wav")), expected);
    assert_eq!(lines, FAKE_STATS);
}
