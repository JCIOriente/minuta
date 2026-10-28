//! The `join` step: one `joined.<ext>` from the sources, in the order given.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::probe::{read_probe, SourceProbe, PROBE_FILE};
use crate::error::{Error, Result};
use crate::job::{write_atomic, Ctx, Step, StepName};
use crate::tools::{run, RunOpts, ToolInfo};

/// File name of the concat demuxer list inside the job folder.
const CONCAT_FILE: &str = "concat.txt";

/// Extensions that keep their container when several sources share them.
const JOIN_EXTENSIONS: [&str; 9] = [
    "mp4", "m4v", "mov", "mkv", "webm", "mp3", "wav", "m4a", "flac",
];

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
/// the result is `joined.mkv`, which holds any stream.
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
            .unwrap_or("mkv"),
        _ => "mkv",
    };
    ctx.dir.join(format!("joined.{ext}"))
}

/// The ffmpeg muxer for an output extension; unknown extensions get matroska.
fn muxer_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
    {
        "mp4" | "m4v" => "mp4",
        "mov" => "mov",
        "webm" => "webm",
        "mp3" => "mp3",
        "wav" => "wav",
        "m4a" => "ipod",
        "flac" => "flac",
        _ => "matroska",
    }
}

/// The audio encoder that fits the container of `path` when only audio is written.
fn audio_codec_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
    {
        "wav" => "pcm_s16le",
        "flac" => "flac",
        "mp3" => "libmp3lame",
        "webm" => "libopus",
        _ => "aac",
    }
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

    fn ffmpeg(
        &self,
        args: Vec<OsString>,
        opts: &RunOpts,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<()> {
        let mut argv = vec![
            self.ffmpeg.path.clone().into_os_string(),
            "-hide_banner".into(),
            "-nostdin".into(),
            "-y".into(),
            // Progress only, so not part of `params`: stats on stderr, one `\r` update
            // every half second (ffmpeg's default period).
            "-stats".into(),
        ];
        argv.extend(args);
        run(&argv, opts.clone(), on_line)?;
        Ok(())
    }

    fn stream_copy(
        &self,
        ctx: &Ctx,
        out: &Path,
        tmp: &Path,
        opts: &RunOpts,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<()> {
        let list_text = concat_list(&ctx.sources)?;
        let list = ctx.dir.join(CONCAT_FILE);
        write_atomic(&list, |t| {
            fs::write(t, &list_text)?;
            Ok(())
        })?;
        self.ffmpeg(
            vec![
                "-f".into(),
                "concat".into(),
                "-safe".into(),
                "0".into(),
                "-i".into(),
                list.into_os_string(),
                "-c".into(),
                "copy".into(),
                "-f".into(),
                muxer_for(out).into(),
                tmp.as_os_str().to_owned(),
            ],
            opts,
            on_line,
        )
    }

    fn reencode(
        &self,
        probes: &[SourceProbe],
        sources: &[PathBuf],
        out: &Path,
        tmp: &Path,
        opts: &RunOpts,
        on_line: &mut dyn FnMut(&str),
    ) -> Result<()> {
        let videos = probes.iter().filter(|p| p.video.is_some()).count();
        let mut argv = Vec::new();
        let mut filter = String::new();
        let mut maps = Vec::new();
        if videos == 0 {
            // Audio only: match the first source's rate and channel layout.
            let first = &probes[0].audio;
            let layout = if first.channels == 1 {
                "mono"
            } else {
                "stereo"
            };
            for (i, source) in sources.iter().enumerate() {
                argv.extend(["-i".into(), source.as_os_str().to_owned()]);
                filter.push_str(&format!(
                    "[{i}:a]aresample={rate},aformat=sample_fmts=fltp:sample_rates={rate}:\
                     channel_layouts={layout}[a{i}];",
                    rate = first.sample_rate
                ));
            }
            for i in 0..probes.len() {
                filter.push_str(&format!("[a{i}]"));
            }
            filter.push_str(&format!("concat=n={}:v=0:a=1[a]", probes.len()));
            maps.extend(["-map".into(), "[a]".into()]);
            maps.extend(["-c:a".into(), audio_codec_for(out).into()]);
        } else if videos == probes.len() {
            let target = probes[0].video.as_ref().expect("all sources have video");
            // Even dimensions, as H.264 needs.
            let (w, h) = (target.width & !1, target.height & !1);
            for (i, source) in sources.iter().enumerate() {
                argv.extend(["-i".into(), source.as_os_str().to_owned()]);
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
            maps.extend(["-map".into(), "[v]".into(), "-map".into(), "[a]".into()]);
            if muxer_for(out) == "webm" {
                maps.extend(["-c:v".into(), "libvpx-vp9".into()]);
                maps.extend(["-crf".into(), "32".into(), "-b:v".into(), "0".into()]);
                maps.extend(["-c:a".into(), "libopus".into()]);
            } else {
                maps.extend(["-c:v".into(), self.encoder.name().into()]);
                match self.encoder {
                    Encoder::VideoToolbox => maps.extend(["-b:v".into(), "5M".into()]),
                    Encoder::X264 => maps.extend([
                        "-crf".into(),
                        "20".into(),
                        "-preset".into(),
                        "veryfast".into(),
                    ]),
                }
                maps.extend(["-pix_fmt".into(), "yuv420p".into()]);
                maps.extend(["-c:a".into(), "aac".into()]);
            }
        } else {
            return Err(Error::Job(
                "cannot join audio-only and video sources".into(),
            ));
        }
        argv.extend(["-filter_complex".into(), filter.into()]);
        argv.extend(maps);
        argv.extend([
            "-f".into(),
            muxer_for(out).into(),
            tmp.as_os_str().to_owned(),
        ]);
        self.ffmpeg(argv, opts, on_line)
    }

    /// Joins the sources, passing every ffmpeg line to `on_line` and stopping with
    /// `Error::Interrupted` when `opts.interrupt` is set. No `joined.<ext>` is left behind
    /// on failure. The runner uses this; [`Step::run`] is the same without progress or
    /// interrupt.
    pub fn run_with(&self, ctx: &Ctx, opts: RunOpts, on_line: &mut dyn FnMut(&str)) -> Result<()> {
        let probes = read_probe(&ctx.dir)?;
        if probes.len() != ctx.sources.len() {
            return Err(Error::Job(format!(
                "{} lists {} sources but the job has {}; run probe again",
                PROBE_FILE,
                probes.len(),
                ctx.sources.len()
            )));
        }
        let out = joined_path(ctx);
        write_atomic(&out, |tmp| match plan_join(&probes) {
            Mode::Link => link_or_copy(&ctx.sources[0], tmp, |s, d| fs::hard_link(s, d)),
            Mode::StreamCopy => self.stream_copy(ctx, &out, tmp, &opts, on_line),
            Mode::Reencode => self.reencode(&probes, &ctx.sources, &out, tmp, &opts, on_line),
        })
    }
}

/// The concat demuxer list for `sources`, one `file '<path>'` line each. A single quote in a
/// path is written as `'\''`. A path that is not UTF-8 or holds a line break cannot be
/// written to the list and is an `Error::Usage`.
fn concat_list(sources: &[PathBuf]) -> Result<String> {
    let mut list = String::new();
    for source in sources {
        match source.to_str() {
            Some(path) if !path.contains(['\n', '\r']) => {
                list.push_str(&format!("file '{}'\n", path.replace('\'', "'\\''")));
            }
            _ => {
                return Err(Error::Usage(format!(
                    "cannot join {}: the path has a line break or is not valid UTF-8",
                    source.display()
                )))
            }
        }
    }
    Ok(list)
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
        self.run_with(ctx, RunOpts::default(), &mut |_| {})
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
        let list = concat_list(&[PathBuf::from("/a/it's.mp4"), PathBuf::from("/b.mp4")]).unwrap();
        assert_eq!(list, "file '/a/it'\\''s.mp4'\nfile '/b.mp4'\n");
    }

    #[test]
    fn concat_list_rejects_line_breaks_and_non_utf8() {
        for bad in ["/a\nb.mp4", "/a\rb.mp4"] {
            let err = concat_list(&[PathBuf::from(bad)]).unwrap_err();
            assert!(matches!(err, Error::Usage(_)), "{bad:?}");
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let bytes = OsString::from_vec(b"/a\xff.mp4".to_vec());
            let err = concat_list(&[PathBuf::from(bytes)]).unwrap_err();
            assert!(matches!(err, Error::Usage(_)));
        }
    }

    #[test]
    fn muxer_follows_the_extension() {
        let muxer = |name: &str| muxer_for(Path::new(name));
        assert_eq!(muxer("joined.mp4"), "mp4");
        assert_eq!(muxer("joined.mov"), "mov");
        assert_eq!(muxer("joined.mkv"), "matroska");
        assert_eq!(muxer("joined.webm"), "webm");
        assert_eq!(muxer("joined.m4a"), "ipod");
        assert_eq!(muxer("joined.wav"), "wav");
        assert_eq!(muxer("joined.xyz"), "matroska");
    }

    #[test]
    fn audio_codec_fits_the_container() {
        let codec = |name: &str| audio_codec_for(Path::new(name));
        assert_eq!(codec("joined.wav"), "pcm_s16le");
        assert_eq!(codec("joined.flac"), "flac");
        assert_eq!(codec("joined.mp3"), "libmp3lame");
        assert_eq!(codec("joined.m4a"), "aac");
    }
}
