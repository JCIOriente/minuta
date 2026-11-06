use std::ffi::OsString;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use crate::error::{Error, Result};
use crate::job::{Ctx, Step};
use crate::steps::probe::{read_probe, Probe, SourceProbe};
#[cfg(unix)]
use crate::steps::transcribe::{Transcribe, TranscribeOutcome};
use crate::tools::{inspect, run, RunOpts, ToolInfo, ToolPaths};

pub(super) struct Tools {
    pub(super) ffmpeg: ToolInfo,
    pub(super) ffprobe: ToolInfo,
}

/// ffmpeg and ffprobe from `PATH`, or `None` (with a note) where they are not installed.
/// Panics instead when `MINUTA_REQUIRE_FFMPEG=1`.
pub(super) fn ffmpeg_available() -> Option<Tools> {
    let report = inspect(&ToolPaths::default(), false);
    match (report.ffmpeg, report.ffprobe) {
        (Some(ffmpeg), Some(ffprobe)) => Some(Tools { ffmpeg, ffprobe }),
        _ => {
            assert!(
                std::env::var("MINUTA_REQUIRE_FFMPEG").as_deref() != Ok("1"),
                "MINUTA_REQUIRE_FFMPEG=1 but ffmpeg and ffprobe are not on PATH"
            );
            eprintln!("skipping: ffmpeg and ffprobe are not on PATH");
            None
        }
    }
}

pub(super) fn argv(tool: &ToolInfo, args: &[&str]) -> Vec<OsString> {
    let mut v = vec![tool.path.clone().into_os_string()];
    v.extend(args.iter().map(OsString::from));
    v
}

pub(super) fn ffmpeg(tools: &Tools, args: &[&str]) {
    let mut full = vec!["-hide_banner", "-nostdin", "-y"];
    full.extend_from_slice(args);
    run(&argv(&tools.ffmpeg, &full), RunOpts::default(), &mut |_| {}).unwrap();
}

/// A tiny mpeg4 + aac clip of a solid `color`, `size` (`WxH`) and `secs` seconds.
pub(super) fn clip(
    tools: &Tools,
    dir: &Path,
    name: &str,
    color: &str,
    size: &str,
    secs: u32,
) -> PathBuf {
    let path = dir.join(name);
    let video = format!("color=c={color}:s={size}:r=25:d={secs}");
    let audio = format!("sine=frequency=440:d={secs}");
    ffmpeg(
        tools,
        &[
            "-f",
            "lavfi",
            "-i",
            &video,
            "-f",
            "lavfi",
            "-i",
            &audio,
            "-c:v",
            "mpeg4",
            "-c:a",
            "aac",
            "-shortest",
            path.to_str().unwrap(),
        ],
    );
    path
}

pub(super) fn ctx(dir: &Path, sources: &[PathBuf]) -> Ctx {
    Ctx {
        dir: dir.to_path_buf(),
        sources: sources
            .iter()
            .map(|s| fs::canonicalize(s).unwrap())
            .collect(),
        source_digest: "digest".into(),
    }
}

pub(super) fn probe_all(t: &Tools, dir: &Path, sources: &[PathBuf]) -> Ctx {
    let ctx = ctx(dir, sources);
    Probe::new(t.ffprobe.clone()).run(&ctx).unwrap();
    ctx
}

/// Probes one finished file, using a scratch job folder of its own.
pub(super) fn describe(t: &Tools, file: &Path) -> SourceProbe {
    let scratch = tempfile::tempdir().unwrap();
    probe_all(t, scratch.path(), &[file.to_path_buf()]);
    read_probe(scratch.path()).unwrap().remove(0)
}

/// Every file under `dir`, relative to it and sorted.
pub(super) fn tree(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push(path.strip_prefix(root).unwrap().display().to_string());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// Committed fakes: `ffmpeg` writes its argv into its output file and prints two
/// `\r`-terminated stats updates; `whisper-cli` writes its argv to `<prefix>.args`.
#[cfg(unix)]
const FIXTURE_TOOLS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tools");

#[cfg(unix)]
pub(super) fn fake_tool(name: &str) -> ToolInfo {
    ToolInfo {
        path: Path::new(FIXTURE_TOOLS).join(name),
        version: format!("{name} 0.0-fake"),
    }
}

#[cfg(unix)]
pub(super) const FAKE_STATS: [&str; 2] = [
    "frame=1 time=00:00:01.00 speed=1x",
    "frame=2 time=00:00:02.00 speed=1x",
];

#[cfg(unix)]
pub(super) fn lines_of(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(String::from)
        .collect()
}

#[cfg(unix)]
pub(super) fn as_strings(items: &[&Path]) -> Vec<String> {
    items
        .iter()
        .map(|p| p.to_str().unwrap().to_string())
        .collect()
}

#[cfg(unix)]
pub(super) const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/whisper");

/// Writes a fake `whisper-cli` that records its argv to `args.txt` next to it and, honouring
/// `-of <prefix>`, copies `fixture` (or nothing, if `None`) to `<prefix>.json`.
#[cfg(unix)]
fn fake_whisper(dir: &Path, fixture: Option<&str>, exit: i32) -> PathBuf {
    let copy = match fixture {
        Some(f) => format!("cp '{FIXTURES}/{f}' \"$prefix.json\""),
        None => String::new(),
    };
    let body = format!(
        r#"#!/bin/sh
printf '%s\n' "$@" > '{args}'
prefix=
while [ $# -gt 0 ]; do
  if [ "$1" = "-of" ]; then prefix=$2; fi
  shift
done
echo "progress = 50%" >&2
{copy}
exit {exit}
"#,
        args = dir.join("args.txt").display()
    );
    let path = dir.join("whisper-cli");
    fs::write(&path, body).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
pub(super) fn tool(path: PathBuf, version: &str) -> ToolInfo {
    ToolInfo {
        path,
        version: version.into(),
    }
}

#[cfg(unix)]
pub(super) struct Setup {
    pub(super) _tmp: tempfile::TempDir,
    pub(super) bin: PathBuf,
    pub(super) model: PathBuf,
    pub(super) ctx: Ctx,
}

#[cfg(unix)]
pub(super) fn setup(fixture: Option<&str>, exit: i32) -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    let job = tmp.path().join("job");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&job).unwrap();
    let bin = fake_whisper(&bin_dir, fixture, exit);
    let model = tmp.path().join("ggml-tiny.bin");
    fs::write(&model, b"model-bytes").unwrap();
    fs::create_dir_all(job.join("assets")).unwrap();
    fs::write(job.join("assets/audio.wav"), b"RIFF").unwrap();
    let ctx = Ctx {
        dir: job,
        sources: vec![],
        source_digest: "d".into(),
    };
    Setup {
        _tmp: tmp,
        bin,
        model,
        ctx,
    }
}

/// The sorted file names in `dir`.
#[cfg(unix)]
pub(super) fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[cfg(unix)]
pub(super) fn step(s: &Setup, lang: &str) -> Transcribe {
    Transcribe::new(tool(s.bin.clone(), "1.9.5"), s.model.clone(), lang, None)
}

/// Retries while the freshly written script is still held open by a fork in another test
/// thread (ETXTBSY on Linux).
#[cfg(unix)]
pub(super) fn run_step(
    t: &Transcribe,
    ctx: &Ctx,
    on_line: &mut dyn FnMut(&str),
) -> Result<TranscribeOutcome> {
    let mut attempts = 0;
    loop {
        let res = t.run_with(ctx, RunOpts::default(), on_line);
        let busy = matches!(&res, Err(Error::Io(e)) if e.to_string().contains("Text file busy"));
        attempts += 1;
        if !busy || attempts >= 20 {
            return res;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
