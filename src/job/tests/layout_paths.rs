use std::path::Path;

use crate::job::layout;

#[test]
fn layout_paths_are_under_minuta_and_assets() {
    let dir = Path::new("/jobs/talk");
    assert_eq!(layout::ASSETS_DIR, "assets");
    assert_eq!(layout::STATE_DIR, ".minuta");
    let expected = [
        (layout::state_file(dir), "/jobs/talk/.minuta/job.json"),
        (layout::probe_file(dir), "/jobs/talk/.minuta/probe.json"),
        (layout::call_cache_dir(dir), "/jobs/talk/.minuta/llm-cache"),
        (
            layout::implicit_md(dir, "youtube"),
            "/jobs/talk/assets/youtube.md",
        ),
        (
            layout::raw_whisper(dir),
            "/jobs/talk/.minuta/whisper-raw.json",
        ),
        (
            layout::transcript_json(dir),
            "/jobs/talk/assets/transcript.json",
        ),
        (layout::audio_file(dir), "/jobs/talk/assets/audio.wav"),
        (
            layout::joined_file(dir, "mp4"),
            "/jobs/talk/assets/joined.mp4",
        ),
        (
            layout::joined_file(dir, "mkv"),
            "/jobs/talk/assets/joined.mkv",
        ),
        (
            layout::template_json(dir, "youtube"),
            "/jobs/talk/assets/youtube.json",
        ),
    ];
    for (path, want) in expected {
        assert_eq!(path, Path::new(want));
    }
}

#[test]
fn call_cache_lives_in_minuta() {
    let root = tempfile::tempdir().unwrap();
    let cache = crate::llm::call::CallCache::new(root.path());
    assert_eq!(
        cache.dir(),
        root.path().join(".minuta").join("llm-cache").as_path()
    );
}
