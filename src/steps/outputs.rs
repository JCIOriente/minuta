//! The `outputs` step: renders `transcript.txt` and `captions.srt` from `transcript.json`.

use std::fs;
use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::job::{write_atomic, Ctx, Step, StepName};
use crate::steps::transcribe::TRANSCRIPT_FILE;
use crate::transcript::{to_srt, to_txt, Transcript};

pub const TXT_FILE: &str = "transcript.txt";
pub const SRT_FILE: &str = "captions.srt";

/// Reads only `transcript.json`, never whisper's raw output.
#[derive(Debug, Clone, Copy, Default)]
pub struct Outputs;

impl Step for Outputs {
    const NAME: StepName = StepName::Outputs;

    fn params(&self, _ctx: &Ctx) -> Vec<u8> {
        Vec::new()
    }

    fn outputs(&self, ctx: &Ctx) -> Vec<PathBuf> {
        vec![ctx.dir.join(TXT_FILE), ctx.dir.join(SRT_FILE)]
    }

    fn run(&self, ctx: &Ctx) -> Result<()> {
        let json = fs::read_to_string(ctx.dir.join(TRANSCRIPT_FILE))?;
        let transcript: Transcript = serde_json::from_str(&json)
            .map_err(|e| Error::Parse(format!("{TRANSCRIPT_FILE}: {e}")))?;
        for (name, text) in [
            (TXT_FILE, to_txt(&transcript)),
            (SRT_FILE, to_srt(&transcript)),
        ] {
            write_atomic(&ctx.dir.join(name), |tmp| {
                fs::write(tmp, &text)?;
                Ok(())
            })?;
        }
        Ok(())
    }
}
