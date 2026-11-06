use crate::job::Step;
use crate::steps::audio::Audio;
use crate::steps::join::{Encoder, Join};
use crate::steps::probe::Probe;
use crate::steps::test_support::{clip, ctx, ffmpeg_available};
use crate::tools::RunOpts;

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
