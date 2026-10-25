#![cfg(unix)]

//! Opt-in: `run -t summary` and `run -t youtube` against the real local Ollama. Set
//! `MINUTA_E2E_LLM=1`; the model is `ollama:gemma4:26b` unless `MINUTA_E2E_LLM_MODEL` names
//! another. The transcript comes from the fake whisper, so no whisper model is needed; ffmpeg
//! and a running Ollama are.

use std::fs;
use std::process::{Command, Stdio};

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Runs `minuta run clip.mp4 -t <template>` against the real Ollama and returns the job folder,
/// with the temp dir that holds it (it is deleted on drop). `None` (after a note) unless
/// `MINUTA_E2E_LLM=1`.
fn run_real(template: &str) -> Option<(tempfile::TempDir, String)> {
    if std::env::var("MINUTA_E2E_LLM").as_deref() != Ok("1") {
        eprintln!("skipping: set MINUTA_E2E_LLM=1 to run `-t {template}` against the real Ollama");
        return None;
    }
    assert!(have("ffmpeg") && have("ffprobe"), "ffmpeg/ffprobe needed");

    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(tmp.path().join("config.toml"), "").unwrap();
    fs::write(tmp.path().join("ggml-test.bin"), "model").unwrap();
    let clip = tmp.path().join("clip.mp4");
    let made = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg("sine=frequency=440:duration=1.5")
        .arg(&clip)
        .status()
        .unwrap();
    assert!(made.success(), "ffmpeg could not generate the clip");

    let model = match std::env::var("MINUTA_E2E_LLM_MODEL") {
        Ok(m) if !m.is_empty() => m,
        _ => "ollama:gemma4:26b".to_string(),
    };
    let out = Command::new(assert_cmd::cargo::cargo_bin("minuta"))
        .args(["run", "clip.mp4", "-t", template, "--llm", &model])
        .current_dir(tmp.path())
        // No MINUTA_* from the developer's shell may redirect the run.
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("MINUTA_CONFIG", tmp.path().join("config.toml"))
        .env(
            "MINUTA_WHISPER_BIN",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/bin/whisper-cli"
            ),
        )
        .env("MINUTA_WHISPER_MODEL", tmp.path().join("ggml-test.bin"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let job = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Some((tmp, job))
}

#[test]
fn e2e_llm_summary_with_real_ollama() {
    let Some((_tmp, job)) = run_real("summary") else {
        return;
    };
    let summary = fs::read_to_string(format!("{job}/summary.md")).unwrap();
    assert!(!summary.trim().is_empty(), "summary.md is empty");
}

#[test]
fn e2e_youtube_limits() {
    let Some((_tmp, job)) = run_real("youtube") else {
        return;
    };
    let artifact: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(format!("{job}/youtube.json")).unwrap()).unwrap();
    assert_eq!(artifact["version"], 1);
    assert_eq!(artifact["template"], "youtube");
    let fields = &artifact["fields"];
    let text = |key: &str| {
        fields[key]
            .as_str()
            .unwrap_or_else(|| panic!("{key}: {fields}"))
    };
    assert!(!text("title").is_empty());
    assert!(text("title").chars().count() <= 100, "title: {fields}");
    assert!(!text("body").is_empty());
    assert!(text("body").chars().count() <= 5000, "body: {fields}");
    assert!(
        text("thumbnail_text").chars().count() <= 40,
        "thumbnail: {fields}"
    );
    let tags: Vec<&str> = fields["tags"]
        .as_array()
        .unwrap_or_else(|| panic!("tags: {fields}"))
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    assert!(!tags.is_empty() && tags.len() <= 15, "tags: {fields}");
    assert!(tags.join(", ").chars().count() <= 500, "tags: {fields}");
    assert!(!fs::read_to_string(format!("{job}/youtube.md"))
        .unwrap()
        .trim()
        .is_empty());
}
