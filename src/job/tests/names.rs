use std::path::Path;

use crate::error::Error;
use crate::job::{derive_name, StepName};

#[test]
fn derive_name_strips_extension() {
    assert_eq!(derive_name(Path::new("clase-4.mp4")).unwrap(), "clase-4");
    assert_eq!(
        derive_name(Path::new("/videos/clase-4.mp4")).unwrap(),
        "clase-4"
    );
    assert_eq!(derive_name(Path::new("clase-4")).unwrap(), "clase-4");
    assert_eq!(derive_name(Path::new("a.b.mkv")).unwrap(), "a.b");
    assert_eq!(derive_name(Path::new("x.mp4.wav")).unwrap(), "x");
    for ext in [
        "mp4", "mkv", "mov", "m4v", "webm", "mp3", "wav", "m4a", "flac",
    ] {
        assert_eq!(
            derive_name(Path::new(&format!("talk.{ext}"))).unwrap(),
            "talk"
        );
    }
    assert_eq!(derive_name(Path::new("notes.txt")).unwrap(), "notes.txt");
}

#[test]
fn derive_name_case_insensitive() {
    assert_eq!(derive_name(Path::new("CLASE-4.MP4")).unwrap(), "CLASE-4");
    assert_eq!(derive_name(Path::new("talk.Mov")).unwrap(), "talk");
    assert_eq!(derive_name(Path::new("talk.FlAc")).unwrap(), "talk");
}

#[test]
fn derive_name_without_file_name_is_usage_error() {
    for path in ["", "..", "/", "videos/.."] {
        assert!(
            matches!(derive_name(Path::new(path)), Err(Error::Usage(_))),
            "{path:?}"
        );
    }
}

#[test]
fn derive_name_rejects_bare_mp4() {
    let err = derive_name(Path::new(".mp4")).unwrap_err();
    assert_eq!(
        err.to_string(),
        "usage error: cannot derive a job name from `.mp4`: \
         the file name is only a media extension"
    );
}

#[test]
fn derive_name_rejects_bare_mkv() {
    let err = derive_name(Path::new("/videos/.mkv")).unwrap_err();
    assert_eq!(
        err.to_string(),
        "usage error: cannot derive a job name from `/videos/.mkv`: \
         the file name is only a media extension"
    );
}

#[test]
fn derive_name_rejects_stacked_bare_extensions() {
    let err = derive_name(Path::new(".mp4.wav")).unwrap_err();
    assert_eq!(
        err.to_string(),
        "usage error: cannot derive a job name from `.mp4.wav`: \
         the file name is only a media extension"
    );
}

#[test]
fn derive_name_keeps_other_dotfiles() {
    assert_eq!(derive_name(Path::new(".notes")).unwrap(), ".notes");
}

#[test]
fn step_names_parse_in_pipeline_order() {
    let names: Vec<StepName> = ["probe", "join", "audio", "transcribe", "outputs"]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(names, StepName::ALL);
    assert!(names.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(StepName::Transcribe.to_string(), "transcribe");
    assert!(matches!("render".parse::<StepName>(), Err(Error::Usage(_))));
}
