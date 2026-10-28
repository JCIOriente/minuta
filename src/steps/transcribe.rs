//! The `transcribe` step: runs `whisper-cli` on `audio.wav` and writes `transcript.json`.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::error::Result;
use crate::job::{write_atomic, Ctx, Step, StepName};
use crate::tools::{run, RunOpts, ToolInfo};
use crate::transcript::{from_whisper_json, Transcript};

/// Input produced by the audio step.
pub const AUDIO_FILE: &str = "audio.wav";
/// Artifact written by this step.
pub const TRANSCRIPT_FILE: &str = "transcript.json";
/// Output prefix handed to whisper's `-of`; whisper writes `<prefix>.json`.
const RAW_PREFIX: &str = "whisper-raw";

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
    /// detection (`--vad -vm <path>`; whisper-cli's `--vad` needs a model). The spec wants VAD
    /// on by default, so the caller (T9/config) must supply one; `None` runs without VAD.
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
        let prefix = ctx.dir.join(RAW_PREFIX);
        let raw = ctx.dir.join(format!("{RAW_PREFIX}.json"));
        let _ = fs::remove_file(&raw);
        let result = self.transcribe(ctx, &prefix, &raw, opts, on_line);
        let _ = fs::remove_file(&raw);
        let transcript = result?;

        write_atomic(&ctx.dir.join(TRANSCRIPT_FILE), |tmp| {
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
            ctx.dir.join(AUDIO_FILE).into(),
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
        from_whisper_json(&fs::read_to_string(raw)?)
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
        vec![ctx.dir.join(TRANSCRIPT_FILE)]
    }

    /// Runs without progress output or interrupt; warnings are dropped. The runner uses
    /// [`Transcribe::run_with`] instead.
    fn run(&self, ctx: &Ctx) -> Result<()> {
        self.run_with(ctx, RunOpts::default(), &mut |_| {})
            .map(|_| ())
    }
}
