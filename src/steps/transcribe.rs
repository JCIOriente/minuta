//! The `transcribe` step: runs `whisper-cli` on `audio.wav` and writes `transcript.json`.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::error::{Error, Result};
use crate::job::{layout, write_atomic_in, Ctx, Step, StepName};
use crate::tools::{run, RunOpts, ToolInfo};
use crate::transcript::{from_whisper_json, Transcript};

/// Artifact written by this step, relative to the job folder. Prefer
/// [`layout::transcript_json`]; this stays for callers that join it to the job folder.
pub const TRANSCRIPT_FILE: &str = "assets/transcript.json";

/// What a successful transcription reports besides its artifact.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TranscribeOutcome {
    /// Human-readable warnings, e.g. when no speech was found. The artifact is still written.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Transcribe {
    whisper: ToolInfo,
    model: PathBuf,
    lang: String,
    vad_model: Option<PathBuf>,
}

impl Transcribe {
    /// `lang` is a whisper language code or `auto`. `vad_model` enables voice activity
    /// detection (`--vad -vm <path>`; whisper-cli's `--vad` needs a model). The runner passes
    /// the resolved `whisper.vad_model`; `None` (no VAD model configured) runs without VAD.
    pub fn new(
        whisper: ToolInfo,
        model: PathBuf,
        lang: impl Into<String>,
        vad_model: Option<PathBuf>,
    ) -> Self {
        Transcribe {
            whisper,
            model,
            lang: lang.into(),
            vad_model,
        }
    }

    /// Runs the step, passing whisper's output lines to `on_line` and honouring
    /// `opts.interrupt`. Zero segments is not an error: the empty transcript is written and
    /// a warning is returned in the outcome. On any failure no `transcript.json` is written
    /// and whisper's raw file is removed.
    pub fn run_with(
        &self,
        ctx: &Ctx,
        opts: RunOpts,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<TranscribeOutcome> {
        let raw = layout::raw_whisper(&ctx.dir);
        // whisper-cli appends `.json` to its output prefix.
        let prefix = raw.with_extension("");
        if let Some(parent) = raw.parent() {
            crate::job::reject_symlinked_tier(parent)?;
            fs::create_dir_all(parent)?;
        }
        let _ = fs::remove_file(&raw);
        let result = self.transcribe(ctx, &prefix, &raw, opts, on_line);
        let _ = fs::remove_file(&raw);
        let transcript = result?;

        write_atomic_in(&ctx.dir, &layout::transcript_json(&ctx.dir), |tmp| {
            let json = serde_json::to_string_pretty(&transcript)
                .map_err(|e| crate::error::Error::Parse(e.to_string()))?;
            fs::write(tmp, json)?;
            Ok(())
        })?;

        let mut outcome = TranscribeOutcome::default();
        if transcript.segments.is_empty() {
            outcome
                .warnings
                .push("no speech detected: the transcript is empty".into());
        }
        Ok(outcome)
    }

    fn transcribe(
        &self,
        ctx: &Ctx,
        prefix: &Path,
        raw: &Path,
        opts: RunOpts,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<Transcript> {
        let mut argv: Vec<OsString> = vec![
            self.whisper.path.clone().into(),
            "-m".into(),
            self.model.clone().into(),
            "-f".into(),
            layout::audio_file(&ctx.dir).into(),
            "-l".into(),
            self.lang.clone().into(),
            "-oj".into(),
            "-of".into(),
            prefix.into(),
            // Progress lines on stderr; not part of `params`, it does not change the result.
            "-pp".into(),
        ];
        if let Some(vad) = &self.vad_model {
            argv.extend(["--vad".into(), "-vm".into(), vad.clone().into()]);
        }
        run(&argv, opts, on_line)?;
        let json = match fs::read_to_string(raw) {
            Ok(json) => json,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(Error::Job(format!(
                    "whisper exited without writing {}",
                    raw.display()
                )))
            }
            Err(e) => {
                return Err(Error::Io(io::Error::new(
                    e.kind(),
                    format!("{}: {e}", raw.display()),
                )))
            }
        };
        from_whisper_json(&json)
    }
}

/// `path`, size and mtime: cheap, and changes when the file is replaced.
fn file_identity(path: &Path) -> String {
    match fs::metadata(path) {
        Ok(m) => {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            format!("{}|{}|{mtime}", path.display(), m.len())
        }
        Err(_) => format!("{}|missing", path.display()),
    }
}

impl Step for Transcribe {
    const NAME: StepName = StepName::Transcribe;

    fn params(&self, _ctx: &Ctx) -> Vec<u8> {
        let vad = self
            .vad_model
            .as_deref()
            .map_or_else(|| "off".to_string(), file_identity);
        format!(
            "model={}\nlang={}\nvad={vad}\nwhisper={}\n",
            file_identity(&self.model),
            self.lang,
            self.whisper.version
        )
        .into_bytes()
    }

    fn outputs(&self, ctx: &Ctx) -> Vec<PathBuf> {
        vec![layout::transcript_json(&ctx.dir)]
    }

    /// Runs without progress output or interrupt; warnings are dropped. The runner uses
    /// [`Transcribe::run_with`] instead.
    fn run(&self, ctx: &Ctx) -> Result<()> {
        self.run_with(ctx, RunOpts::default(), &mut |_| {})
            .map(|_| ())
    }
}
