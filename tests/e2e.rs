#![cfg(unix)]

//! Opt-in: the real `whisper-cli` with a real model. Set `MINUTA_E2E=1`; give a model file
//! with `MINUTA_E2E_MODEL`, or the test runs `minuta models pull tiny` into a temp cache.

use std::fs;
use std::process::{Command, Stdio};

fn have(tool: &str, flag: &str) -> bool {
    Command::new(tool)
        .arg(flag)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn e2e_real_whisper_tiny() {
    if std::env::var("MINUTA_E2E").as_deref() != Ok("1") {
        eprintln!("skipping: set MINUTA_E2E=1 to run the real whisper-cli end to end");
        return;
    }
    assert!(
        have("ffmpeg", "-version") && have("ffprobe", "-version"),
        "ffmpeg/ffprobe needed"
    );
    assert!(
        have("whisper-cli", "--version") || have("whisper-cli", "-h"),
        "whisper-cli needed"
    );

    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(tmp.path().join("config.toml"), "").unwrap();
    let minuta = |args: &[&str]| {
        let mut cmd = Command::new(assert_cmd::cargo::cargo_bin("minuta"));
        cmd.args(args)
            .current_dir(tmp.path())
            // No MINUTA_* from the developer's shell may swap in another whisper or model.
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("MINUTA_CONFIG", tmp.path().join("config.toml"))
            .stdin(Stdio::null());
        cmd
    };

    let model = match std::env::var("MINUTA_E2E_MODEL") {
        Ok(m) if !m.is_empty() => m,
        _ => {
            let pull = minuta(&["models", "pull", "tiny"]).output().unwrap();
            assert!(
                pull.status.success(),
                "models pull tiny failed: {}",
                String::from_utf8_lossy(&pull.stderr)
            );
            "tiny".to_string()
        }
    };

    let clip = tmp.path().join("tone.wav");
    let ok = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=2",
        ])
        .arg(&clip)
        .status()
        .unwrap()
        .success();
    assert!(ok, "ffmpeg could not generate the clip");

    let out = minuta(&["run", "tone.wav", "--model", &model])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let job = String::from_utf8(out.stdout).unwrap();
    let job = std::path::Path::new(job.trim());
    let json = fs::read_to_string(job.join("transcript.json")).unwrap();
    serde_json::from_str::<serde_json::Value>(&json).expect("transcript.json parses");
    assert!(job.join("captions.srt").is_file());
}
