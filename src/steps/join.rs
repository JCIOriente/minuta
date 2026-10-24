//! The `join` step: one `joined.<ext>` from the sources, in the order given.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::probe::{read_probe, SourceProbe};
use crate::error::{Error, Result};
use crate::job::{write_atomic, Ctx, Step, StepName};
use crate::tools::{run, RunOpts, ToolInfo};

/// File name of the concat demuxer list inside the job folder.
const CONCAT_FILE: &str = "concat.txt";

/// Extensions that keep their container when several sources are joined.
const JOIN_EXTENSIONS: [&str; 8] = ["mp4", "m4v", "mov", "mkv", "mp3", "wav", "m4a", "flac"];

/// The H.264 encoder used when sources have to be re-encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoder {
    VideoToolbox,
    X264,
}

impl Encoder {
    /// VideoToolbox when the output of `ffmpeg -encoders` lists it, else libx264.
    pub fn pick(listing: &str) -> Encoder {
        let has_videotoolbox = listing
            .lines()
            .any(|l| l.split_whitespace().nth(1) == Some("h264_videotoolbox"));
        if has_videotoolbox {
            Encoder::VideoToolbox
        } else {
            Encoder::X264
        }
    }

    /// The ffmpeg encoder name.
    pub fn name(self) -> &'static str {
        match self {
            Encoder::VideoToolbox => "h264_videotoolbox",
            Encoder::X264 => "libx264",
        }
    }
}

/// How the sources become one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// One source: hard link, or a copy where linking is impossible.
    Link,
    /// Several sources with matching streams: concat demuxer with `-c copy`.
    StreamCopy,
    /// Several sources that differ: decode and encode again.
    Reencode,
}

/// Decides the [`Mode`] from the probe results. Streams match when codec, resolution and
/// frame rate agree, and so do the audio codec, sample rate and channel count, which stream
/// copy needs as much as the video parameters.
pub fn plan_join(probes: &[SourceProbe]) -> Mode {
    match probes {
        [] | [_] => Mode::Link,
        [first, rest @ ..] => {
            if rest
                .iter()
                .all(|p| p.video == first.video && p.audio == first.audio)
            {
                Mode::StreamCopy
            } else {
                Mode::Reencode
            }
        }
    }
}

/// Where the joined file goes: `joined.<ext>` in the job folder. One source keeps its own
/// extension. Several keep the extension they share when it is a known container, else
/// the result is `joined.mp4`.
pub fn joined_path(ctx: &Ctx) -> PathBuf {
    let exts: Vec<String> = ctx
        .sources
        .iter()
        .map(|s| {
            s.extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default()
        })
        .collect();
    let ext = match exts.as_slice() {
        [one] if !one.is_empty() => one.as_str(),
        [first, rest @ ..] if rest.iter().all(|e| e == first) => JOIN_EXTENSIONS
            .iter()
            .find(|j| **j == first)
            .copied()
            .unwrap_or("mp4"),
        _ => "mp4",
    };
    ctx.dir.join(format!("joined.{ext}"))
}

/// The ffmpeg muxer for an output extension.
fn format_for(path: &Path) -> Result<&'static str> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    Ok(match ext {
        "mp4" | "m4v" => "mp4",
        "mov" => "mov",
        "mkv" => "matroska",
        "mp3" => "mp3",
        "wav" => "wav",
        "m4a" => "ipod",
        "flac" => "flac",
        other => return Err(Error::Usage(format!("cannot join into a .{other} file"))),
    })
}

/// Joins the sources into `joined.<ext>`.
#[derive(Debug, Clone)]
pub struct Join {
    ffmpeg: ToolInfo,
    encoder: Encoder,
}

impl Join {
    pub fn new(ffmpeg: ToolInfo, encoder: Encoder) -> Self {
        Join { ffmpeg, encoder }
    }

    /// A `Join` that re-encodes with the best encoder this ffmpeg offers.
    pub fn detect(ffmpeg: ToolInfo) -> Result<Self> {
        let argv = [
            ffmpeg.path.clone().into_os_string(),
            "-hide_banner".into(),
            "-encoders".into(),
        ];
        let mut listing = String::new();
        run(&argv, RunOpts::default(), &mut |line| {
            listing.push_str(line);
            listing.push('\n');
        })?;
        let encoder = Encoder::pick(&listing);
        Ok(Join { ffmpeg, encoder })
    }

    fn ffmpeg(&self, args: Vec<OsString>) -> Result<()> {
        let mut argv = vec![
            self.ffmpeg.path.clone().into_os_string(),
            "-hide_banner".into(),
            "-nostdin".into(),
            "-y".into(),
        ];
        argv.extend(args);
        run(&argv, RunOpts::default(), &mut |_| {})?;
        Ok(())
    }

    fn stream_copy(&self, ctx: &Ctx, out: &Path, tmp: &Path) -> Result<()> {
        let list = ctx.dir.join(CONCAT_FILE);
        write_atomic(&list, |t| {
            fs::write(t, concat_list(&ctx.sources))?;
            Ok(())
        })?;
        self.ffmpeg(vec![
            "-f".into(),
            "concat".into(),
            "-safe".into(),
            "0".into(),
            "-i".into(),
            list.into_os_string(),
            "-c".into(),
            "copy".into(),
            "-f".into(),
            format_for(out)?.into(),
            tmp.as_os_str().to_owned(),
        ])
    }

    fn reencode(&self, probes: &[SourceProbe], out: &Path, tmp: &Path) -> Result<()> {
        let target = match probes[0].video.as_ref() {
            Some(video) if probes.iter().all(|p| p.video.is_some()) => video,
            _ => {
                return Err(Error::Usage(
                    "cannot join sources that differ when some are audio only".into(),
                ))
            }
        };
        // Even dimensions, as H.264 needs.
        let (w, h) = (target.width & !1, target.height & !1);
        let mut argv = Vec::new();
        let mut filter = String::new();
        for (i, p) in probes.iter().enumerate() {
            argv.extend(["-i".into(), p.path.as_os_str().to_owned()]);
            filter.push_str(&format!(
                "[{i}:v]scale={w}:{h}:force_original_aspect_ratio=decrease,\
                 pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps={fps}[v{i}];\
                 [{i}:a]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo[a{i}];",
                fps = target.fps
            ));
        }
        for i in 0..probes.len() {
            filter.push_str(&format!("[v{i}][a{i}]"));
        }
        filter.push_str(&format!("concat=n={}:v=1:a=1[v][a]", probes.len()));
        argv.extend([
            "-filter_complex".into(),
            filter.into(),
            "-map".into(),
            "[v]".into(),
            "-map".into(),
            "[a]".into(),
            "-c:v".into(),
            self.encoder.name().into(),
        ]);
        match self.encoder {
            Encoder::VideoToolbox => argv.extend(["-b:v".into(), "5M".into()]),
            Encoder::X264 => argv.extend([
                "-crf".into(),
                "20".into(),
                "-preset".into(),
                "veryfast".into(),
            ]),
        }
        argv.extend([
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-c:a".into(),
            "aac".into(),
            "-f".into(),
            format_for(out)?.into(),
            tmp.as_os_str().to_owned(),
        ]);
        self.ffmpeg(argv)
    }
}

/// The concat demuxer list for `sources`, one `file '<path>'` line each. A single quote in a
/// path is written as `'\''`.
fn concat_list(sources: &[PathBuf]) -> String {
    sources
        .iter()
        .map(|s| format!("file '{}'\n", s.to_string_lossy().replace('\'', "'\\''")))
        .collect()
}

/// Hard-links `src` to `dst` with `link`, copying when that fails (across file systems, for
/// example). `dst` is replaced if it exists.
fn link_or_copy(
    src: &Path,
    dst: &Path,
    link: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> Result<()> {
    match fs::remove_file(dst) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    if link(src, dst).is_err() {
        fs::copy(src, dst)?;
    }
    Ok(())
}

impl Step for Join {
    const NAME: StepName = StepName::Join;

    fn params(&self, ctx: &Ctx) -> Vec<u8> {
        let mut params = format!(
            "ffmpeg={}\nencoder={}\n",
            self.ffmpeg.version,
            self.encoder.name()
        );
        for source in &ctx.sources {
            params.push_str(&format!("source={}\n", source.display()));
        }
        params.into_bytes()
    }

    fn outputs(&self, ctx: &Ctx) -> Vec<PathBuf> {
        vec![joined_path(ctx)]
    }

    fn run(&self, ctx: &Ctx) -> Result<()> {
        let probes = read_probe(&ctx.dir)?;
        let out = joined_path(ctx);
        write_atomic(&out, |tmp| match plan_join(&probes) {
            Mode::Link => link_or_copy(&ctx.sources[0], tmp, |s, d| fs::hard_link(s, d)),
            Mode::StreamCopy => self.stream_copy(ctx, &out, tmp),
            Mode::Reencode => self.reencode(&probes, &out, tmp),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_failure_falls_back_to_copy() {
        let dir = tempfile::tempdir().unwrap();
        let (src, dst) = (dir.path().join("a.mp4"), dir.path().join("b.mp4"));
        fs::write(&src, "media").unwrap();
        fs::write(&dst, "stale").unwrap();

        link_or_copy(&src, &dst, |_, _| Err(io::Error::other("cross-device"))).unwrap();

        assert_eq!(fs::read_to_string(&dst).unwrap(), "media");
    }

    #[test]
    fn concat_list_escapes_single_quotes() {
        let list = concat_list(&[PathBuf::from("/a/it's.mp4"), PathBuf::from("/b.mp4")]);
        assert_eq!(list, "file '/a/it'\\''s.mp4'\nfile '/b.mp4'\n");
    }
}
