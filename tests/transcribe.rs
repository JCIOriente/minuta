#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use minuta::error::{Error, Result};
use minuta::job::{Ctx, Step};
use minuta::steps::outputs::Outputs;
use minuta::steps::transcribe::{Transcribe, TranscribeOutcome};
use minuta::tools::{RunOpts, ToolInfo};
use minuta::transcript::{self, Segment, Transcript};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/whisper");

/// Writes a fake `whisper-cli` that records its argv to `args.txt` next to it and, honouring
/// `-of <prefix>`, copies `fixture` (or nothing, if `None`) to `<prefix>.json`.
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

fn tool(path: PathBuf, version: &str) -> ToolInfo {
    ToolInfo {
        path,
        version: version.into(),
    }
}

struct Setup {
    _tmp: tempfile::TempDir,
    bin: PathBuf,
    model: PathBuf,
    ctx: Ctx,
}

fn setup(fixture: Option<&str>, exit: i32) -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = tmp.path().join("bin");
    let job = tmp.path().join("job");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&job).unwrap();
    let bin = fake_whisper(&bin_dir, fixture, exit);
    let model = tmp.path().join("ggml-tiny.bin");
    fs::write(&model, b"model-bytes").unwrap();
    fs::write(job.join("audio.wav"), b"RIFF").unwrap();
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

fn step(s: &Setup, lang: &str) -> Transcribe {
    Transcribe::new(tool(s.bin.clone(), "1.9.5"), s.model.clone(), lang, None)
}

/// Retries while the freshly written script is still held open by a fork in another test
/// thread (ETXTBSY on Linux).
fn run_step(t: &Transcribe, ctx: &Ctx, on_line: &mut dyn FnMut(&str)) -> Result<TranscribeOutcome> {
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

fn args_of(s: &Setup) -> Vec<String> {
    fs::read_to_string(s.bin.parent().unwrap().join("args.txt"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect()
}

fn pair<'a>(args: &'a [String], flag: &str) -> &'a str {
    let i = args.iter().position(|a| a == flag).unwrap();
    &args[i + 1]
}

#[test]
fn whisper_args_include_vad_and_json() {
    let s = setup(Some("sample.json"), 0);
    let vad = s.model.with_file_name("ggml-silero.bin");
    let t = Transcribe::new(
        tool(s.bin.clone(), "1.9.5"),
        s.model.clone(),
        "auto",
        Some(vad.clone()),
    );
    let mut lines = Vec::new();
    run_step(&t, &s.ctx, &mut |l| lines.push(l.to_string())).unwrap();

    let args = args_of(&s);
    assert!(args.contains(&"--vad".to_string()));
    assert_eq!(pair(&args, "-vm"), vad.to_str().unwrap());
    assert!(args.contains(&"-oj".to_string()));
    assert_eq!(pair(&args, "-m"), s.model.to_str().unwrap());
    assert_eq!(pair(&args, "-l"), "auto");
    assert_eq!(
        pair(&args, "-f"),
        s.ctx.dir.join("audio.wav").to_str().unwrap()
    );
    let prefix = Path::new(pair(&args, "-of"));
    assert_eq!(prefix.parent().unwrap(), s.ctx.dir);
    assert!(lines.iter().any(|l| l.contains("progress = 50%")));

    // The parsed transcript is written, whisper's raw file is gone, and no temp is left.
    let json = fs::read_to_string(s.ctx.dir.join("transcript.json")).unwrap();
    let t: Transcript = serde_json::from_str(&json).unwrap();
    assert!(!t.segments.is_empty());
    let mut names: Vec<_> = fs::read_dir(&s.ctx.dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["audio.wav", "transcript.json"]);
}

#[test]
fn no_vad_model_omits_vad() {
    let s = setup(Some("sample.json"), 0);
    run_step(&step(&s, "es"), &s.ctx, &mut |_| {}).unwrap();
    let args = args_of(&s);
    assert!(!args.contains(&"--vad".to_string()));
    assert_eq!(pair(&args, "-l"), "es");
}

#[test]
fn lang_change_changes_params() {
    let s = setup(None, 0);
    let a = step(&s, "auto").params(&s.ctx);
    assert_eq!(a, step(&s, "auto").params(&s.ctx));
    assert_ne!(a, step(&s, "es").params(&s.ctx));
}

#[test]
fn model_change_changes_params() {
    let s = setup(None, 0);
    let base = step(&s, "auto").params(&s.ctx);

    // Same path, different size.
    fs::write(&s.model, b"other-model-bytes").unwrap();
    let resized = step(&s, "auto").params(&s.ctx);
    assert_ne!(base, resized);

    // Different path.
    let other = s.model.with_file_name("ggml-base.bin");
    fs::copy(&s.model, &other).unwrap();
    let moved = Transcribe::new(tool(s.bin.clone(), "1.9.5"), other, "auto", None);
    assert_ne!(resized, moved.params(&s.ctx));

    // Different whisper version.
    let newer = Transcribe::new(tool(s.bin.clone(), "1.9.6"), s.model.clone(), "auto", None);
    assert_ne!(resized, newer.params(&s.ctx));

    // VAD on versus off, and a different VAD model.
    let vad = |p: &str| {
        Transcribe::new(
            tool(s.bin.clone(), "1.9.5"),
            s.model.clone(),
            "auto",
            Some(PathBuf::from(p)),
        )
        .params(&s.ctx)
    };
    assert_ne!(resized, vad("/v1"));
    assert_ne!(vad("/v1"), vad("/v2"));
}

#[test]
fn empty_transcript_still_writes_outputs() {
    let s = setup(Some("empty.json"), 0);
    let outcome = run_step(&step(&s, "auto"), &s.ctx, &mut |_| {}).unwrap();
    assert_eq!(outcome.warnings.len(), 1);
    assert!(outcome.warnings[0].contains("no speech"));

    let json = fs::read_to_string(s.ctx.dir.join("transcript.json")).unwrap();
    let t: Transcript = serde_json::from_str(&json).unwrap();
    assert!(t.segments.is_empty());

    Outputs.run(&s.ctx).unwrap();
    assert_eq!(
        fs::read_to_string(s.ctx.dir.join("transcript.txt")).unwrap(),
        ""
    );
    assert_eq!(
        fs::read_to_string(s.ctx.dir.join("captions.srt")).unwrap(),
        ""
    );
}

#[test]
fn whisper_failure_leaves_no_artifact() {
    let s = setup(None, 3);
    let err = run_step(&step(&s, "auto"), &s.ctx, &mut |_| {}).unwrap_err();
    assert!(matches!(err, Error::Tool { code: Some(3), .. }), "{err:?}");
    assert!(!s.ctx.dir.join("transcript.json").exists());
    assert!(!s.ctx.dir.join("transcript.json.tmp").exists());

    // Also when whisper "succeeds" but wrote garbage.
    let s = setup(Some("sample.json"), 0);
    fs::write(&s.bin, "#!/bin/sh\nwhile [ $# -gt 0 ]; do [ \"$1\" = -of ] && p=$2; shift; done\necho nope > \"$p.json\"\n").unwrap();
    let err = run_step(&step(&s, "auto"), &s.ctx, &mut |_| {}).unwrap_err();
    assert!(matches!(err, Error::Parse(_)), "{err:?}");
    assert!(!s.ctx.dir.join("transcript.json").exists());
    assert_eq!(fs::read_dir(&s.ctx.dir).unwrap().count(), 1);
}

#[test]
fn missing_whisper_output_names_file() {
    // Exits 0 but writes no `<prefix>.json`.
    let s = setup(None, 0);
    let err = run_step(&step(&s, "auto"), &s.ctx, &mut |_| {}).unwrap_err();

    let expected = format!(
        "whisper exited without writing {}",
        s.ctx.dir.join("whisper-raw.json").display()
    );
    assert!(matches!(&err, Error::Job(m) if *m == expected), "{err}");
    let names: Vec<_> = fs::read_dir(&s.ctx.dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["audio.wav"]);
}

#[test]
fn outputs_derive_from_transcript_json() {
    let s = setup(None, 0);
    let t = Transcript {
        language: "en".into(),
        segments: vec![Segment {
            start: 0.0,
            end: 1.5,
            text: "only in transcript json".into(),
        }],
    };
    fs::write(
        s.ctx.dir.join("transcript.json"),
        serde_json::to_string(&t).unwrap(),
    )
    .unwrap();
    // A raw whisper file with different content must be ignored.
    fs::copy(
        format!("{FIXTURES}/sample.json"),
        s.ctx.dir.join("whisper-raw.json"),
    )
    .unwrap();

    assert_eq!(
        Outputs.outputs(&s.ctx),
        [
            s.ctx.dir.join("transcript.txt"),
            s.ctx.dir.join("captions.srt")
        ]
    );
    Outputs.run(&s.ctx).unwrap();
    assert_eq!(
        fs::read_to_string(s.ctx.dir.join("transcript.txt")).unwrap(),
        transcript::to_txt(&t)
    );
    assert_eq!(
        fs::read_to_string(s.ctx.dir.join("captions.srt")).unwrap(),
        transcript::to_srt(&t)
    );
}

#[test]
fn outputs_fail_without_transcript_json() {
    let s = setup(None, 0);
    assert!(Outputs.run(&s.ctx).is_err());
    assert!(!s.ctx.dir.join("transcript.txt").exists());
    assert!(!s.ctx.dir.join("captions.srt").exists());
}

#[cfg(unix)]
#[test]
fn outputs_do_not_follow_a_planted_tmp_symlink() {
    let s = setup(None, 0);
    let t = Transcript {
        language: "en".into(),
        segments: vec![Segment {
            start: 0.0,
            end: 1.5,
            text: "new text".into(),
        }],
    };
    fs::write(
        s.ctx.dir.join("transcript.json"),
        serde_json::to_string(&t).unwrap(),
    )
    .unwrap();
    let victim = s._tmp.path().join("victim.txt");
    fs::write(&victim, "precious").unwrap();
    for name in ["transcript.txt.tmp", "captions.srt.tmp"] {
        std::os::unix::fs::symlink(&victim, s.ctx.dir.join(name)).unwrap();
    }

    Outputs.run(&s.ctx).unwrap();

    assert_eq!(fs::read_to_string(&victim).unwrap(), "precious");
    assert_eq!(
        fs::read_to_string(s.ctx.dir.join("transcript.txt")).unwrap(),
        transcript::to_txt(&t)
    );
    assert_eq!(
        fs::read_to_string(s.ctx.dir.join("captions.srt")).unwrap(),
        transcript::to_srt(&t)
    );
}
