use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::job::{Ctx, Step};
use crate::steps::outputs::Outputs;
use crate::steps::test_support::{
    as_strings, fake_tool, lines_of, names_in, run_step, setup, step, tool, Setup,
};
use crate::steps::transcribe::Transcribe;
use crate::tools::RunOpts;
use crate::transcript::Transcript;

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
        s.ctx.dir.join("assets/audio.wav").to_str().unwrap()
    );
    let prefix = Path::new(pair(&args, "-of"));
    assert_eq!(prefix.parent().unwrap(), s.ctx.dir.join(".minuta"));
    assert!(lines.iter().any(|l| l.contains("progress = 50%")));

    // The parsed transcript is written, whisper's raw file is gone, and no temp is left.
    let json = fs::read_to_string(s.ctx.dir.join("assets/transcript.json")).unwrap();
    let t: Transcript = serde_json::from_str(&json).unwrap();
    assert!(!t.segments.is_empty());
    assert_eq!(
        names_in(&s.ctx.dir.join("assets")),
        ["audio.wav", "transcript.json"]
    );
    assert_eq!(names_in(&s.ctx.dir.join(".minuta")), Vec::<String>::new());
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

    let json = fs::read_to_string(s.ctx.dir.join("assets/transcript.json")).unwrap();
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
    assert!(!s.ctx.dir.join("assets/transcript.json").exists());
    assert!(!s.ctx.dir.join("assets/transcript.json.tmp").exists());

    // Also when whisper "succeeds" but wrote garbage.
    let s = setup(Some("sample.json"), 0);
    fs::write(&s.bin, "#!/bin/sh\nwhile [ $# -gt 0 ]; do [ \"$1\" = -of ] && p=$2; shift; done\necho nope > \"$p.json\"\n").unwrap();
    let err = run_step(&step(&s, "auto"), &s.ctx, &mut |_| {}).unwrap_err();
    assert!(matches!(err, Error::Parse(_)), "{err:?}");
    assert!(!s.ctx.dir.join("assets/transcript.json").exists());
    assert_eq!(names_in(&s.ctx.dir.join("assets")), ["audio.wav"]);
    assert_eq!(names_in(&s.ctx.dir.join(".minuta")), Vec::<String>::new());
}

#[test]
fn missing_whisper_output_names_file() {
    // Exits 0 but writes no `<prefix>.json`.
    let s = setup(None, 0);
    let err = run_step(&step(&s, "auto"), &s.ctx, &mut |_| {}).unwrap_err();

    let expected = format!(
        "whisper exited without writing {}",
        s.ctx.dir.join(".minuta/whisper-raw.json").display()
    );
    assert!(matches!(&err, Error::Job(m) if *m == expected), "{err}");
    assert_eq!(names_in(&s.ctx.dir.join("assets")), ["audio.wav"]);
}

#[cfg(unix)]
#[test]
fn whisper_argv_asks_for_progress_and_forwards_it() {
    use crate::steps::transcribe::Transcribe;
    let tmp = tempfile::tempdir().unwrap();
    let model = tmp.path().join("ggml-tiny.bin");
    fs::write(&model, b"model").unwrap();
    fs::create_dir(tmp.path().join("assets")).unwrap();
    fs::write(tmp.path().join("assets/audio.wav"), b"RIFF").unwrap();
    let ctx = Ctx {
        dir: tmp.path().to_path_buf(),
        sources: vec![],
        source_digest: "d".into(),
    };
    let mut lines = Vec::new();
    Transcribe::new(fake_tool("whisper-cli"), model.clone(), "en", None)
        .run_with(&ctx, RunOpts::default(), &mut |l| lines.push(l.to_string()))
        .unwrap();

    let prefix = tmp.path().join(".minuta/whisper-raw");
    let audio = tmp.path().join("assets/audio.wav");
    let mut expected = vec!["-m".to_string()];
    expected.extend(as_strings(&[&model]));
    expected.push("-f".into());
    expected.extend(as_strings(&[&audio]));
    expected.extend(["-l", "en", "-oj", "-of"].map(String::from));
    expected.extend(as_strings(&[&prefix]));
    expected.push("-pp".into());
    assert_eq!(
        lines_of(&tmp.path().join(".minuta/whisper-raw.args")),
        expected
    );
    assert_eq!(
        lines,
        vec!["whisper_print_progress_callback: progress =  50%"]
    );
}
