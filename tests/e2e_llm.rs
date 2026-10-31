#![cfg(unix)]

//! Opt-in: `run -t summary` and `run -t youtube` (which also runs `chapters`, whose files go to `assets/`) against the real
//! local Ollama. Set `MINUTA_E2E_LLM=1`; the model is `ollama:gemma4:26b` unless
//! `MINUTA_E2E_LLM_MODEL` names another. The transcript comes from the fake whisper, so no
//! whisper model is needed; ffmpeg and a running Ollama are.

use std::fs;
use std::process::{Command, Stdio};

const CLASS_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/whisper/class.json"
);

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Runs `minuta run clip.mp4 -t <template>` against the real Ollama and returns the job folder,
/// with the temp dir that holds it (it is deleted on drop). `fixture` is the whisper JSON the
/// fake whisper emits (`None`: its default, `sample.json`). `None` (after a note) unless
/// `MINUTA_E2E_LLM=1`.
fn run_real(template: &str, fixture: Option<&str>) -> Option<(tempfile::TempDir, String)> {
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
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin("minuta"));
    cmd.args(["run", "clip.mp4", "-t", template, "--llm", &model])
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
        .stdin(Stdio::null());
    if let Some(fixture) = fixture {
        cmd.env("FAKE_WHISPER_FIXTURE", fixture);
    }
    let out = cmd.output().unwrap();
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
    let Some((_tmp, job)) = run_real("summary", None) else {
        return;
    };
    let summary = fs::read_to_string(format!("{job}/summary.md")).unwrap();
    assert!(!summary.trim().is_empty(), "summary.md is empty");
    assert_eq!(
        empty_section_lines(&summary),
        Vec::<&str>::new(),
        "{summary}"
    );
}

/// Lines of `summary` that belong to a section the default fixture (`sample.json`: a greeting
/// and "let's start the meeting") has nothing for, or that are placeholders for missing content,
/// in English or Spanish. The summary prompt says to omit such sections entirely (#115).
fn empty_section_lines(summary: &str) -> Vec<&str> {
    const SECTION_WORDS: [&str; 7] = [
        "decision",
        "decisión",
        "action item",
        "open question",
        "tareas",
        "acciones",
        "preguntas",
    ];
    const PLACEHOLDER_STARTS: [&str; 7] = [
        "no se ", "no hay ", "no hubo ", "ninguna", "ninguno", "none", "nothing ",
    ];
    summary
        .lines()
        .filter(|line| {
            let lower = line.to_lowercase();
            let text = lower.trim_start_matches(['#', '*', '-', ' ']);
            SECTION_WORDS.iter().any(|w| lower.contains(w))
                || PLACEHOLDER_STARTS.iter().any(|p| text.starts_with(p))
        })
        .collect()
}

/// `m:ss` below one hour, `h:mm:ss` from one hour (YouTube chapter stamps).
fn stamp(seconds: u64) -> String {
    if seconds < 3600 {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    } else {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    }
}

/// The segment starts of `class.json` in whole seconds (floor), and the transcript end.
fn class_segments() -> (Vec<u64>, u64) {
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(CLASS_FIXTURE).unwrap()).unwrap();
    let segments = json["transcription"].as_array().unwrap();
    let ms = |s: &serde_json::Value, key: &str| s["offsets"][key].as_u64().unwrap();
    let starts = segments.iter().map(|s| ms(s, "from") / 1000).collect();
    let end = ms(segments.last().unwrap(), "to") / 1000;
    (starts, end)
}

/// Checks every chapter rule (ADR-0008) and the built-in limits (20 chapters, 1500
/// characters as rendered) on a `chapters.json` artifact.
fn assert_chapter_rules(artifact: &serde_json::Value) {
    assert_eq!(artifact["version"], 1, "{artifact}");
    let (segment_starts, end) = class_segments();
    let chapters: Vec<(u64, &str)> = artifact["chapters"]
        .as_array()
        .unwrap_or_else(|| panic!("chapters: {artifact}"))
        .iter()
        .map(|c| {
            (
                c["start"].as_u64().unwrap_or_else(|| panic!("start: {c}")),
                c["title"].as_str().unwrap_or_else(|| panic!("title: {c}")),
            )
        })
        .collect();
    assert!(
        (3..=20).contains(&chapters.len()),
        "count {}: {artifact}",
        chapters.len()
    );
    assert_eq!(
        chapters[0].0, 0,
        "the first chapter starts at 0: {artifact}"
    );
    for (i, (start, title)) in chapters.iter().enumerate() {
        assert!(
            segment_starts.contains(start),
            "chapter {}: {start} is not a segment start: {artifact}",
            i + 1
        );
        assert!(
            *start < end,
            "chapter {}: starts after the end: {artifact}",
            i + 1
        );
        let until = chapters.get(i + 1).map_or(end, |next| next.0);
        assert!(
            until >= start + 10,
            "chapter {}: shorter than 10 s or out of order: {artifact}",
            i + 1
        );
        assert!(
            !title.trim().is_empty() && !title.contains('\n'),
            "chapter {}: title: {artifact}",
            i + 1
        );
    }
    let rendered: Vec<String> = chapters
        .iter()
        .map(|(start, title)| format!("{} {title}", stamp(*start)))
        .collect();
    let chars = rendered.join("\n").chars().count();
    assert!(chars <= 1500, "{chars} characters as rendered: {artifact}");
}

#[test]
fn e2e_chapters_and_youtube_limits() {
    let Some((_tmp, job)) = run_real("youtube", Some(CLASS_FIXTURE)) else {
        return;
    };
    let chapters: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(format!("{job}/assets/chapters.json")).unwrap())
            .unwrap();
    assert_chapter_rules(&chapters);

    let artifact: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(format!("{job}/assets/youtube.json")).unwrap())
            .unwrap();
    assert_eq!(artifact["version"], 1);
    assert_eq!(artifact["template"], "youtube");
    let fields = &artifact["fields"];
    assert_eq!(fields["chapters"], chapters["chapters"], "{fields}");
    let text = |key: &str| {
        fields[key]
            .as_str()
            .unwrap_or_else(|| panic!("{key}: {fields}"))
    };
    assert!(!text("title").is_empty());
    assert!(text("title").chars().count() <= 100, "title: {fields}");
    assert!(!text("body").is_empty());
    assert!(text("body").chars().count() <= 3200, "body: {fields}");
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
