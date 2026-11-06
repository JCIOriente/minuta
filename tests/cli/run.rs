use std::fs;

use assert_cmd::assert::OutputAssertExt;
use predicates::str::contains;

use crate::common::*;
use crate::fake_tools::*;

#[test]
fn usage_error_exits_2() {
    let env = FakeEnv::new();
    env.minuta(&[]).assert().code(2);
    env.minuta(&["bogus"]).assert().code(2);
    env.minuta(&["run"]).assert().code(2);
    env.minuta(&["run", "clase-4.mp4", "--frobnicate"])
        .assert()
        .code(2);
    env.minuta(&["models"]).assert().code(2);
    env.minuta(&["config", "nope"]).assert().code(2);
    assert!(env.calls().is_empty());
}

#[test]
fn unknown_force_step_is_usage_error() {
    let env = FakeEnv::new();
    env.minuta(&["run", "clase-4.mp4", "--force", "render"])
        .assert()
        .code(2)
        .stderr(contains("unknown step `render`"));
    assert!(!env.path("minuta-out").exists());
}

#[test]
fn missing_source_file_is_usage_error() {
    let env = FakeEnv::new();
    env.minuta(&["run", "clase-4.mp4", "nope.mp4"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(contains("nope.mp4"));
    assert!(!env.path("minuta-out").exists());
    assert!(env.calls().is_empty());
}

#[test]
fn stdout_is_only_job_path() {
    let env = FakeEnv::new();
    let out = env.minuta(&["run", "clase-4.mp4"]);
    out.clone().assert().success();
    let job = env.job_dir("clase-4");
    assert_eq!(stdout(&out), format!("{}\n", job.display()));
    for artifact in [
        ".minuta/job.json",
        ".minuta/probe.json",
        "assets/audio.wav",
        "assets/transcript.json",
        "transcript.txt",
        "captions.srt",
    ] {
        assert!(job.join(artifact).is_file(), "missing {artifact}");
    }
    assert!(!joined_file(&job).exists());
    assert!(!job.join("joined.mp4").exists());
    assert!(!state_dir(&job).join("whisper-raw.json").exists());
    assert_eq!(
        env.tools_called(),
        ["ffprobe", "ffmpeg", "whisper-cli"],
        "probe, audio and transcribe run their tool; a single source needs no join"
    );
    // Progress, with a step counter and timing, goes to stderr.
    let err = stderr(&out);
    for (i, step) in ["probe", "join", "audio", "transcribe", "outputs"]
        .iter()
        .enumerate()
    {
        assert!(
            err.contains(&format!("[{}/5] {step}", i + 1)),
            "stderr was {err}"
        );
    }
    assert!(err.contains("language=es"), "stderr was {err}");
    assert!(
        err.contains("note: VAD disabled (no VAD model configured)"),
        "stderr was {err}"
    );
}

#[test]
fn name_flag_strips_media_extension() {
    let env = FakeEnv::new();
    let out = env.minuta(&["run", "clase-4.mp4", "-n", "leccion.mp4"]);
    out.clone().assert().success();
    assert_eq!(
        stdout(&out),
        format!("{}\n", env.job_dir("leccion").display())
    );
}

#[test]
fn second_run_all_cached() {
    let env = FakeEnv::new();
    env.minuta(&["run", "clase-4.mp4"]).assert().success();
    let calls = env.calls();

    let out = env.minuta(&["run", "clase-4.mp4"]);
    out.clone().assert().success();
    assert_eq!(env.calls(), calls, "a cached run executes no step");
    let err = stderr(&out);
    assert_eq!(cached_steps(&err).len(), 5, "stderr was {err}");
    assert!(err.contains("[4/5] transcribe  cached"), "stderr was {err}");
    assert!(!err.contains("VAD disabled"), "stderr was {err}");
    assert_eq!(
        stdout(&out),
        format!("{}\n", env.job_dir("clase-4").display())
    );
}

#[test]
fn second_run_skips_all_steps() {
    let env = FakeEnv::new();
    fs::write(env.path("clase-5.mp4"), "more media").unwrap();
    let args = ["run", "clase-4.mp4", "clase-5.mp4", "-n", "both"];
    env.minuta(&args).assert().success();
    let job = env.job_dir("both");
    assert!(joined_file(&job).is_file());
    assert!(!job.join("joined.mp4").exists());
    assert!(!job.join("concat.txt").exists());
    let calls = env.calls();

    let out = env.minuta(&args);
    out.clone().assert().success();

    assert_eq!(env.calls(), calls, "a cached run executes no step");
    assert_eq!(cached_steps(&stderr(&out)).len(), 5, "{}", stderr(&out));
}

#[test]
fn transcribe_behaves_like_run() {
    let env = FakeEnv::new();
    let out = env.minuta(&["transcribe", "clase-4.mp4"]);
    out.clone().assert().success();
    assert_eq!(
        stdout(&out),
        format!("{}\n", env.job_dir("clase-4").display())
    );
    assert!(env.job_dir("clase-4").join("captions.srt").is_file());
    // Same job, so `run` finds every step cached.
    let calls = env.calls();
    env.minuta(&["run", "clase-4.mp4"]).assert().success();
    assert_eq!(env.calls(), calls);
}

#[test]
fn quiet_suppresses_progress() {
    let env = FakeEnv::new();
    let out = env.minuta(&["run", "clase-4.mp4", "--quiet"]);
    out.clone().assert().success().stderr("");
    assert_eq!(
        stdout(&out),
        format!("{}\n", env.job_dir("clase-4").display())
    );
}

#[test]
fn quiet_still_prints_warnings() {
    let env = FakeEnv::new();
    let empty = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/whisper/empty.json"
    );
    env.whisper(&format!("/bin/cp '{empty}' \"$prefix.json\"; exit 0"));
    let out = env.minuta(&["run", "clase-4.mp4", "-q"]);
    out.clone().assert().success();
    assert_eq!(
        stderr(&out),
        "warning: no speech detected: the transcript is empty\n"
    );
}

#[test]
fn tool_failure_exits_1_and_marks_step_stale() {
    let env = FakeEnv::new();
    env.whisper("echo 'model file is corrupt' >&2; exit 3");
    let out = env.minuta(&["run", "clase-4.mp4"]);
    out.clone().assert().code(1).stdout("");
    assert!(stderr(&out).contains("model file is corrupt"));
    let job = env.job_dir("clase-4");
    assert_eq!(step_status(&job, "audio").as_deref(), Some("done"));
    assert_eq!(step_status(&job, "transcribe").as_deref(), Some("stale"));
    assert!(!transcript_json(&job).exists());
}

#[test]
fn failing_encoder_listing_leaves_no_job_folder() {
    let env = FakeEnv::new();
    script(
        &env.bin(),
        "ffmpeg",
        r#"if [ "$1" = "-version" ]; then echo "ffmpeg version 7.1-fake"; exit 0; fi
echo "cannot list encoders" >&2
exit 1"#,
    );
    let out = env.minuta(&["run", "clase-4.mp4"]);
    out.clone().assert().code(1).stdout("");
    assert!(
        stderr(&out).contains("cannot list encoders"),
        "{}",
        stderr(&out)
    );
    assert!(!env.path("minuta-out").exists());
}

#[test]
fn bare_binary_name_is_looked_up_on_path() {
    let env = FakeEnv::new();
    fs::rename(env.bin().join("ffmpeg"), env.bin().join("ffmpeg7")).unwrap();
    let out = env.run_cmd({
        let mut cmd = env.command(&["run", "clase-4.mp4"]);
        cmd.env("MINUTA_FFMPEG_BIN", "ffmpeg7");
        cmd
    });
    out.clone().assert().success();
    assert_eq!(env.tools_called(), ["ffprobe", "ffmpeg7", "whisper-cli"]);
}

#[test]
fn different_sources_without_force_is_refused() {
    let env = FakeEnv::new();
    fs::write(env.path("other.mp4"), "more media").unwrap();
    env.minuta(&["run", "clase-4.mp4"])
        .clone()
        .assert()
        .success();
    let job = env.job_dir("clase-4");
    let state = fs::read_to_string(state_file(&job)).unwrap();
    let calls = env.calls();

    let out = env.minuta(&["run", "clase-4.mp4", "other.mp4"]);
    out.clone().assert().code(1).stdout("");
    assert!(stderr(&out).contains("--force"), "{}", stderr(&out));
    assert_eq!(env.calls(), calls);
    assert_eq!(fs::read_to_string(state_file(&job)).unwrap(), state);
}

#[test]
fn different_sources_with_another_name_is_a_separate_job() {
    let env = FakeEnv::new();
    fs::write(env.path("other.mp4"), "more media").unwrap();
    env.minuta(&["run", "clase-4.mp4"])
        .clone()
        .assert()
        .success();
    let state = fs::read_to_string(state_file(&env.job_dir("clase-4"))).unwrap();

    let out = env.minuta(&["run", "clase-4.mp4", "other.mp4", "--name", "both"]);
    out.clone().assert().success();
    assert_eq!(stdout(&out), format!("{}\n", env.job_dir("both").display()));
    assert_eq!(
        fs::read_to_string(state_file(&env.job_dir("clase-4"))).unwrap(),
        state
    );

    // The same name for yet other sources is refused too: --name never replaces a job.
    env.minuta(&["run", "other.mp4", "--name", "both"])
        .clone()
        .assert()
        .code(1);
}

#[test]
fn different_sources_with_force_replace_the_job() {
    let env = FakeEnv::new();
    fs::write(env.path("other.mp4"), "more media").unwrap();
    env.minuta(&["run", "clase-4.mp4"])
        .clone()
        .assert()
        .success();

    let out = env.minuta(&["run", "clase-4.mp4", "other.mp4", "--force"]);
    out.clone().assert().success();
    let job = env.job_dir("clase-4");
    assert_eq!(stdout(&out), format!("{}\n", job.display()));
    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(state_file(&job)).unwrap()).unwrap();
    assert_eq!(state["sources"].as_array().unwrap().len(), 2);
    assert_eq!(step_status(&job, "outputs").as_deref(), Some("done"));
}
