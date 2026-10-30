//! The `probe` step: ffprobe every source and record what join and audio need to know.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::job::{layout, write_atomic_in, Ctx, Step, StepName};
use crate::tools::{run, RunOpts, ToolInfo};

/// The first video stream of a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoStream {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    /// Frame rate as ffprobe reports it, a fraction such as `25/1` or `30000/1001`.
    pub fps: String,
}

/// The first audio stream of a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioStream {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u32,
}

/// What ffprobe found in one source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceProbe {
    pub path: PathBuf,
    /// Seconds.
    pub duration: f64,
    /// `None` for audio-only sources.
    pub video: Option<VideoStream>,
    pub audio: AudioStream,
}

/// Reads `probe.json` from a job folder: one entry per source, in join order.
pub fn read_probe(dir: &Path) -> Result<Vec<SourceProbe>> {
    let file = layout::probe_file(dir);
    let text = fs::read_to_string(&file)?;
    serde_json::from_str(&text).map_err(|e| Error::Parse(format!("{}: {e}", file.display())))
}

/// Probes every source with ffprobe and writes `probe.json`. A source without an audio
/// stream is an `Error::Job`.
#[derive(Debug, Clone)]
pub struct Probe {
    ffprobe: ToolInfo,
}

impl Probe {
    pub fn new(ffprobe: ToolInfo) -> Self {
        Probe { ffprobe }
    }

    fn probe_one(
        &self,
        path: &Path,
        opts: &RunOpts,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<SourceProbe> {
        let argv: Vec<OsString> = vec![
            self.ffprobe.path.clone().into_os_string(),
            "-v".into(),
            "error".into(),
            "-print_format".into(),
            "json".into(),
            "-show_format".into(),
            "-show_streams".into(),
            path.as_os_str().to_owned(),
        ];
        let mut json = String::new();
        run(&argv, opts.clone(), &mut |line| {
            json.push_str(line);
            json.push('\n');
            on_line(line);
        })?;
        parse(path, &json)
    }

    /// Probes every source and writes `probe.json`, passing every ffprobe line to `on_line`
    /// and stopping with `Error::Interrupted` when `opts.interrupt` is set. The runner uses
    /// this; [`Step::run`] is the same without progress or interrupt.
    pub fn run_with(&self, ctx: &Ctx, opts: RunOpts, on_line: &mut dyn FnMut(&str)) -> Result<()> {
        let probes = ctx
            .sources
            .iter()
            .map(|s| self.probe_one(s, &opts, on_line))
            .collect::<Result<Vec<_>>>()?;
        let json = serde_json::to_string_pretty(&probes)
            .map_err(|e| Error::Parse(format!("cannot encode probe results: {e}")))?;
        write_atomic_in(&ctx.dir, &layout::probe_file(&ctx.dir), |tmp| {
            fs::write(tmp, json)?;
            Ok(())
        })
    }
}

#[derive(Deserialize)]
struct RawProbe {
    #[serde(default)]
    streams: Vec<RawStream>,
    format: Option<RawFormat>,
}

#[derive(Deserialize)]
struct RawFormat {
    duration: Option<String>,
}

#[derive(Deserialize)]
struct RawStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    r_frame_rate: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u32>,
    disposition: Option<RawDisposition>,
}

#[derive(Deserialize)]
struct RawDisposition {
    #[serde(default)]
    attached_pic: u8,
}

fn parse(path: &Path, json: &str) -> Result<SourceProbe> {
    let bad = |what: &str| Error::Parse(format!("ffprobe output for {}: {what}", path.display()));
    let raw: RawProbe = serde_json::from_str(json).map_err(|e| bad(&e.to_string()))?;
    let of_type = |kind: &str| {
        raw.streams
            .iter()
            .filter(|s| s.codec_type.as_deref() == Some(kind))
            .find(|s| !matches!(&s.disposition, Some(d) if d.attached_pic != 0))
    };
    let video = of_type("video")
        .map(|s| {
            Ok::<_, Error>(VideoStream {
                codec: s
                    .codec_name
                    .clone()
                    .ok_or_else(|| bad("video codec missing"))?,
                width: s.width.ok_or_else(|| bad("video width missing"))?,
                height: s.height.ok_or_else(|| bad("video height missing"))?,
                fps: s
                    .r_frame_rate
                    .clone()
                    .ok_or_else(|| bad("frame rate missing"))?,
            })
        })
        .transpose()?;
    let audio = of_type("audio")
        .ok_or_else(|| Error::Job(format!("no audio stream in {}", path.display())))?;
    let audio = AudioStream {
        codec: audio
            .codec_name
            .clone()
            .ok_or_else(|| bad("audio codec missing"))?,
        sample_rate: audio
            .sample_rate
            .as_deref()
            .and_then(|r| r.parse().ok())
            .ok_or_else(|| bad("sample rate missing"))?,
        channels: audio.channels.ok_or_else(|| bad("channel count missing"))?,
    };
    let duration = raw
        .format
        .and_then(|f| f.duration)
        .and_then(|d| d.parse().ok())
        .ok_or_else(|| bad("duration missing"))?;
    Ok(SourceProbe {
        path: path.to_path_buf(),
        duration,
        video,
        audio,
    })
}

impl Step for Probe {
    const NAME: StepName = StepName::Probe;

    fn params(&self, ctx: &Ctx) -> Vec<u8> {
        let mut params = format!("ffprobe={}\n", self.ffprobe.version);
        for source in &ctx.sources {
            params.push_str(&format!("source={}\n", source.display()));
        }
        params.into_bytes()
    }

    fn outputs(&self, ctx: &Ctx) -> Vec<PathBuf> {
        vec![layout::probe_file(&ctx.dir)]
    }

    fn run(&self, ctx: &Ctx) -> Result<()> {
        self.run_with(ctx, RunOpts::default(), &mut |_| {})
    }
}
