//! The VAD model: how `run` passes it to whisper, catalog names, and the preflight errors.

use std::fs;
use std::path::Path;

use assert_cmd::assert::OutputAssertExt;

use crate::common::*;
use crate::fake_tools::*;

/// The whisper invocation `run` makes for job `name`, with `vad` appended when given.
fn whisper_call(env: &FakeEnv, name: &str, vad: Option<&Path>) -> String {
    let job = env.job_dir(name);
    let mut call = format!(
        "whisper-cli -m {} -f {} -l auto -oj -of {} -pp",
        env.path("ggml-test.bin").display(),
        audio_file(&job).display(),
        state_dir(&job).join("whisper-raw").display()
    );
    if let Some(vad) = vad {
        call.push_str(&format!(" --vad -vm {}", vad.display()));
    }
    call
}

#[test]
fn vad_model_path_runs_whisper_with_vad() {
    let env = FakeEnv::new();
    let vad = env.path("vad.bin");
    fs::write(&vad, "vad").unwrap();
    let out = env.run_cmd({
        let mut cmd = env.command(&["run", "clase-4.mp4"]);
        cmd.env("MINUTA_WHISPER_VAD_MODEL", &vad);
        cmd
    });
    out.clone().assert().success();
    let whisper: Vec<String> = env
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("whisper-cli "))
        .collect();
    assert_eq!(whisper, [whisper_call(&env, "clase-4", Some(&vad))]);
    let err = stderr(&out);
    assert!(!err.contains("VAD disabled"), "stderr was {err}");
}

#[test]
fn vad_model_catalog_name_resolves_into_models_dir() {
    let env = FakeEnv::new();
    let dir = env.path("home/.minuta/models/vad");
    fs::create_dir_all(&dir).unwrap();
    let vad = dir.join("ggml-silero-v5.1.2.bin");
    fs::write(&vad, "vad").unwrap();
    fs::write(
        env.path("config.toml"),
        "[whisper]\nvad_model = \"silero-v5.1.2\"\n",
    )
    .unwrap();
    env.minuta(&["run", "clase-4.mp4"]).assert().success();
    let whisper: Vec<String> = env
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("whisper-cli "))
        .collect();
    assert_eq!(whisper, [whisper_call(&env, "clase-4", Some(&vad))]);
}

#[test]
fn without_vad_model_whisper_runs_without_vad() {
    let env = FakeEnv::new();
    env.minuta(&["run", "clase-4.mp4"]).assert().success();
    let whisper: Vec<String> = env
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("whisper-cli "))
        .collect();
    assert_eq!(whisper, [whisper_call(&env, "clase-4", None)]);
}

#[test]
fn vad_model_not_pulled_hints_pull() {
    let env = FakeEnv::new();
    let out = env.run_cmd({
        let mut cmd = env.command(&["run", "clase-4.mp4"]);
        cmd.env("MINUTA_WHISPER_VAD_MODEL", "silero-v5.1.2");
        cmd
    });
    out.clone().assert().code(1).stdout("");
    assert_eq!(
        stderr(&out),
        "minuta: missing required tools: vad model\n  \
         vad model: run `minuta models pull silero-v5.1.2`\n"
    );
    assert!(env.calls().is_empty());
    assert!(!env.path("minuta-out").exists());
}

#[test]
fn vad_model_missing_path_is_preflight_error() {
    let env = FakeEnv::new();
    let vad = env.path("no-such-vad.bin");
    let out = env.run_cmd({
        let mut cmd = env.command(&["run", "clase-4.mp4"]);
        cmd.env("MINUTA_WHISPER_VAD_MODEL", &vad);
        cmd
    });
    out.clone().assert().code(1).stdout("");
    assert_eq!(
        stderr(&out),
        format!(
            "minuta: missing required tools: vad model\n  \
             vad model: configured path {} is not a file\n",
            vad.display()
        )
    );
    assert!(env.calls().is_empty());
}

#[test]
fn whisper_model_rejects_the_vad_catalog_name() {
    let env = FakeEnv::new();
    let out = env.run_cmd({
        let mut cmd = env.command(&["run", "clase-4.mp4"]);
        cmd.env("MINUTA_WHISPER_MODEL", "silero-v5.1.2");
        cmd
    });
    out.clone().assert().code(2).stdout("");
    assert_eq!(
        stderr(&out),
        "minuta: usage error: `silero-v5.1.2` is a VAD model; \
         set it as whisper.vad_model, not whisper.model\n"
    );
    assert!(env.calls().is_empty());
}

#[test]
fn vad_model_rejects_a_whisper_catalog_name() {
    let env = FakeEnv::new();
    let out = env.run_cmd({
        let mut cmd = env.command(&["run", "clase-4.mp4"]);
        cmd.env("MINUTA_WHISPER_VAD_MODEL", "tiny");
        cmd
    });
    out.clone().assert().code(2).stdout("");
    assert_eq!(
        stderr(&out),
        "minuta: usage error: `tiny` is a whisper model; \
         set it as whisper.model, not whisper.vad_model\n"
    );
    assert!(env.calls().is_empty());
}

#[test]
fn doctor_reports_missing_vad_model() {
    let env = FakeEnv::new();
    let out = env.run_cmd({
        let mut cmd = env.command(&["doctor"]);
        cmd.env("MINUTA_WHISPER_VAD_MODEL", "silero-v5.1.2");
        cmd
    });
    out.clone().assert().code(1);
    let err = stderr(&out);
    assert!(
        err.ends_with("\n  vad model: run `minuta models pull silero-v5.1.2`\n"),
        "stderr was {err}"
    );
}
