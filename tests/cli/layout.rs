//! The job folder layout end to end: documents at the root, JSON and media in `assets/`,
//! bookkeeping in `.minuta/`. The real binary, real ffmpeg, the fake whisper and fake Ollama.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use fake_ollama::Reply;

use crate::common::*;

/// Names directly inside `dir`, hidden ones included, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// Every file under `dir`, relative and sorted.
fn all_files(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, base, out);
            } else {
                out.push(path.strip_prefix(base).unwrap().display().to_string());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Fake Ollama: valid chapters, a valid `youtube` answer, and a `body`/`hashtags` answer for
/// the other fields templates.
fn social_fake() -> fake_ollama::FakeOllama {
    ollama_with(Arc::new(|req| {
        if is_chapters(req) {
            Reply::chat(&valid_chapters())
        } else if !req.body["format"]["properties"]["title"].is_null() {
            Reply::chat(&valid_youtube())
        } else {
            Reply::chat(r##"{"body":"A post about ownership.","hashtags":["rust"]}"##)
        }
    }))
}

#[test]
fn layout_single_source_social_run() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = social_fake();
    let args = [
        "run",
        "a.mp4",
        "-t",
        "linkedin",
        "-t",
        "youtube",
        "-t",
        "instagram",
    ];
    let out = run_class(&env, &args, &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    let job = env.job_dir("a");
    let listings = (
        names(&job),
        names(&job.join("assets")),
        names(&job.join(".minuta")),
    );
    assert_eq!(
        listings,
        (
            strs(&[
                ".minuta",
                "assets",
                "captions.srt",
                "instagram.md",
                "linkedin.md",
                "transcript.txt",
                "youtube.md",
            ]),
            strs(&[
                "audio.wav",
                "chapters.json",
                "chapters.md",
                "instagram.json",
                "linkedin.json",
                "transcript.json",
                "youtube.json",
            ]),
            strs(&["job.json", "llm-cache", "probe.json"]),
        )
    );
    let stray: Vec<String> = all_files(&job)
        .into_iter()
        .filter(|f| f.contains("joined") || f.contains("summary"))
        .collect();
    assert_eq!(stray, Vec::<String>::new());
    assert_eq!(tmp_files(&job), Vec::<String>::new());

    // A second run skips every step and writes nothing new.
    let before = all_files(&job);
    let chats = fake.chat_count();
    let out = run_class(&env, &args, &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(all_files(&job), before);
    assert_eq!(fake.chat_count(), chats);
}

#[test]
fn layout_two_sources_has_joined_in_assets() {
    require_ffmpeg!();
    let env = Env::new();
    env.clip("a.mp4", 440);
    env.clip("b.mp4", 660);
    env.run_ok(&["run", "a.mp4", "b.mp4", "--name", "both"]);
    let job = env.job_dir("both");
    assert_eq!(
        (names(&job), names(&job.join("assets"))),
        (
            strs(&[".minuta", "assets", "captions.srt", "transcript.txt"]),
            strs(&["audio.wav", "joined.mp4", "transcript.json"]),
        )
    );
    assert_eq!(
        names(&job.join(".minuta")),
        strs(&["job.json", "probe.json"])
    );
}

#[test]
fn layout_chapters_moves_out_of_assets_when_requested() {
    require_ffmpeg!();
    let env = env_with_clip();
    let fake = social_fake();
    let out = run_class(&env, &["run", "a.mp4", "-t", "youtube"], &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    let job = env.job_dir("a");
    let assets = names(&job.join("assets"));
    assert_eq!(
        (
            assets.contains(&"chapters.json".to_string()),
            assets.contains(&"chapters.md".to_string()),
            names(&job).contains(&"chapters.md".to_string()),
        ),
        (true, true, false)
    );
    let out = run_class(&env, &["run", "a.mp4", "-t", "chapters"], &fake);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        (names(&job), names(&job.join("assets"))),
        (
            strs(&[
                ".minuta",
                "assets",
                "captions.srt",
                "chapters.md",
                "transcript.txt",
                "youtube.md",
            ]),
            strs(&[
                "audio.wav",
                "chapters.json",
                "transcript.json",
                "youtube.json",
            ]),
        ),
        "{}",
        stderr(&out)
    );
}
