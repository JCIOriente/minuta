use crate::tools::{inspect, install_hint, ToolPaths};

const INSTALL_GUIDE: &str = "https://github.com/JCIOriente/minuta/blob/main/docs/install.md";

#[test]
fn install_hint_is_a_brew_command_on_macos_and_linux() {
    for os in ["macos", "linux"] {
        assert_eq!(install_hint("ffmpeg", os), "brew install ffmpeg", "{os}");
        assert_eq!(
            install_hint("ffprobe", os),
            "brew install ffmpeg (ffprobe ships with ffmpeg)",
            "{os}"
        );
        assert_eq!(
            install_hint("whisper-cli", os),
            "brew install whisper.cpp",
            "{os}"
        );
    }
}

#[test]
fn install_hint_points_to_the_guide_on_other_systems() {
    for os in ["windows", "freebsd"] {
        for tool in ["ffmpeg", "ffprobe", "whisper-cli"] {
            assert_eq!(
                install_hint(tool, os),
                format!("see {INSTALL_GUIDE}"),
                "{tool} on {os}"
            );
        }
    }
}

/// `inspect` hands each tool its own name, so a missing ffmpeg is never told to install
/// what a missing whisper-cli needs.
#[test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn inspect_gives_each_tool_its_own_install_hint() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ToolPaths {
        path: Some(dir.path().as_os_str().to_owned()),
        ..ToolPaths::default()
    };
    let report = inspect(&paths, false);
    let hints: Vec<(&str, &str)> = report
        .missing
        .iter()
        .map(|m| (m.name.as_str(), m.hint.as_str()))
        .collect();
    assert_eq!(
        hints,
        [
            ("ffmpeg", "brew install ffmpeg"),
            ("ffprobe", "brew install ffmpeg (ffprobe ships with ffmpeg)"),
            ("whisper-cli", "brew install whisper.cpp"),
        ]
    );
}
