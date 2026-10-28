//! The `audio` step: the joined file's sound as mono 16 kHz PCM WAV, which is what whisper reads.

use std::path::PathBuf;

use super::join::joined_path;
use crate::error::Result;
use crate::job::{write_atomic, Ctx, Step, StepName};
use crate::tools::{run, RunOpts, ToolInfo};

/// File name of the audio artifact inside the job folder.
pub const AUDIO_FILE: &str = "audio.wav";

/// Extracts `audio.wav` from the joined file.
#[derive(Debug, Clone)]
pub struct Audio {
    ffmpeg: ToolInfo,
}

impl Audio {
    pub fn new(ffmpeg: ToolInfo) -> Self {
        Audio { ffmpeg }
    }

    /// Extracts `audio.wav`, passing every ffmpeg line to `on_line` and stopping with
    /// `Error::Interrupted` when `opts.interrupt` is set. The runner uses this;
    /// [`Step::run`] is the same without progress or interrupt.
    pub fn run_with(&self, ctx: &Ctx, opts: RunOpts, on_line: &mut dyn FnMut(&str)) -> Result<()> {
        let input = joined_path(ctx);
        write_atomic(&ctx.dir.join(AUDIO_FILE), |tmp| {
            let argv = [
                self.ffmpeg.path.as_os_str(),
                "-hide_banner".as_ref(),
                "-nostdin".as_ref(),
                "-y".as_ref(),
                // Progress only, so not part of `params`: stats on stderr, one `\r`
                // update every half second.
                "-stats".as_ref(),
                "-stats_period".as_ref(),
                "0.5".as_ref(),
                "-i".as_ref(),
                input.as_os_str(),
                "-vn".as_ref(),
                "-ac".as_ref(),
                "1".as_ref(),
                "-ar".as_ref(),
                "16000".as_ref(),
                "-c:a".as_ref(),
                "pcm_s16le".as_ref(),
                "-f".as_ref(),
                "wav".as_ref(),
                tmp.as_os_str(),
            ]
            .map(ToOwned::to_owned);
            run(&argv, opts, on_line)?;
            Ok(())
        })
    }
}

impl Step for Audio {
    const NAME: StepName = StepName::Audio;

    fn params(&self, _ctx: &Ctx) -> Vec<u8> {
        format!(
            "ffmpeg={}\nformat=pcm_s16le mono 16000\n",
            self.ffmpeg.version
        )
        .into_bytes()
    }

    fn outputs(&self, ctx: &Ctx) -> Vec<PathBuf> {
        vec![ctx.dir.join(AUDIO_FILE)]
    }

    fn run(&self, ctx: &Ctx) -> Result<()> {
        self.run_with(ctx, RunOpts::default(), &mut |_| {})
    }
}
