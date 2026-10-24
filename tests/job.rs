use std::path::Path;

use minuta::job::derive_name;

#[test]
fn derive_name_strips_extension() {
    assert_eq!(derive_name(Path::new("clase-4.mp4")), "clase-4");
    assert_eq!(derive_name(Path::new("/videos/clase-4.mp4")), "clase-4");
    assert_eq!(derive_name(Path::new("clase-4")), "clase-4");
    assert_eq!(derive_name(Path::new("a.b.mkv")), "a.b");
    assert_eq!(derive_name(Path::new("x.mp4.wav")), "x");
    for ext in [
        "mp4", "mkv", "mov", "m4v", "webm", "mp3", "wav", "m4a", "flac",
    ] {
        assert_eq!(derive_name(Path::new(&format!("talk.{ext}"))), "talk");
    }
    assert_eq!(derive_name(Path::new("notes.txt")), "notes.txt");
}

#[test]
fn derive_name_case_insensitive() {
    assert_eq!(derive_name(Path::new("CLASE-4.MP4")), "CLASE-4");
    assert_eq!(derive_name(Path::new("talk.Mov")), "talk");
    assert_eq!(derive_name(Path::new("talk.FlAc")), "talk");
}
