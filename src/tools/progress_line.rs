/// A progress report recognized in one line of a tool's output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ProgressLine {
    /// An ffmpeg `-stats` line; `time` is its `time=` field in seconds, `None` for
    /// `time=N/A`.
    FfmpegStats { time: Option<f64> },
    /// A whisper-cli `-pp` line: the percent done.
    Whisper { percent: u32 },
}

/// What whisper-cli `-pp` prints before the percent (whisper.cpp 1.9.5:
/// `whisper_print_progress_callback: progress =  23%`).
const WHISPER_PROGRESS_PREFIX: &str = "whisper_print_progress_callback: progress =";

/// The progress report in `line`, if it is one. An ffmpeg stats line starts with `frame=`
/// (video) or `size=` (audio only) and has a `time=` field.
pub(crate) fn progress_line(line: &str) -> Option<ProgressLine> {
    let line = line.trim();
    if let Some(rest) = line.strip_prefix(WHISPER_PROGRESS_PREFIX) {
        let percent = rest.trim_start().strip_suffix('%')?.parse().ok()?;
        return Some(ProgressLine::Whisper { percent });
    }
    if !(line.starts_with("frame=") || line.starts_with("size=")) {
        return None;
    }
    let time = line
        .split_whitespace()
        .find_map(|field| field.strip_prefix("time="))?;
    Some(ProgressLine::FfmpegStats {
        time: clock_seconds(time),
    })
}

/// Seconds in ffmpeg's `HH:MM:SS.ss` clock; `None` for anything else, such as `N/A`. ffmpeg
/// prints a negative time before the first timestamp, which counts as zero.
fn clock_seconds(clock: &str) -> Option<f64> {
    if let Some(positive) = clock.strip_prefix('-') {
        return clock_seconds(positive).map(|_| 0.0);
    }
    let mut parts = clock.splitn(3, ':');
    let hours: u32 = parts.next()?.parse().ok()?;
    let minutes: u32 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some(f64::from(hours) * 3600.0 + f64::from(minutes) * 60.0 + seconds)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn progress_line_ffmpeg_video_stats() {
        assert_eq!(
            progress_line(
                "frame=  366 fps=0.0 q=28.0 size=     256KiB time=00:00:16.99 \
                 bitrate= 123.4kbits/s speed=33.7x elapsed=0:00:00.50"
            ),
            Some(ProgressLine::FfmpegStats { time: Some(16.99) })
        );
    }

    #[test]
    fn progress_line_ffmpeg_final_stats() {
        assert_eq!(
            progress_line(
                "frame= 1500 fps=906 q=-1.0 Lsize=    1116KiB time=00:01:00.00 \
                 bitrate= 152.4kbits/s speed=36.2x elapsed=0:00:01.65"
            ),
            Some(ProgressLine::FfmpegStats { time: Some(60.0) })
        );
    }

    #[test]
    fn progress_line_ffmpeg_audio_only_stats() {
        assert_eq!(
            progress_line(
                "size=     938KiB time=00:00:30.00 bitrate= 256.0kbits/s \
                 speed=2.35e+03x elapsed=0:00:00.01"
            ),
            Some(ProgressLine::FfmpegStats { time: Some(30.0) })
        );
    }

    #[test]
    fn progress_line_ffmpeg_hours() {
        assert_eq!(
            progress_line("size=  1KiB time=01:02:03.50 bitrate=N/A speed=1x"),
            Some(ProgressLine::FfmpegStats { time: Some(3723.5) })
        );
    }

    #[test]
    fn progress_line_ffmpeg_negative_time_is_zero() {
        assert_eq!(
            progress_line("frame=    0 fps=0.0 q=0.0 size=       0KiB time=-00:00:00.02 speed=N/A"),
            Some(ProgressLine::FfmpegStats { time: Some(0.0) })
        );
    }

    #[test]
    fn progress_line_ffmpeg_time_not_available() {
        assert_eq!(
            progress_line(
                "frame=    0 fps=0.0 q=0.0 size=       0KiB time=N/A bitrate=N/A speed=N/A"
            ),
            Some(ProgressLine::FfmpegStats { time: None })
        );
    }

    #[test]
    fn progress_line_whisper() {
        assert_eq!(
            progress_line("whisper_print_progress_callback: progress =  23%"),
            Some(ProgressLine::Whisper { percent: 23 })
        );
    }

    #[test]
    fn progress_line_whisper_hundred() {
        assert_eq!(
            progress_line("whisper_print_progress_callback: progress = 100%"),
            Some(ProgressLine::Whisper { percent: 100 })
        );
    }

    #[test]
    fn progress_line_ignores_frame_without_time() {
        assert_eq!(progress_line("frame=2"), None);
    }

    #[test]
    fn progress_line_ignores_messages() {
        assert_eq!(progress_line("Error opening output file out.mp4."), None);
    }

    #[test]
    fn progress_line_ignores_other_progress_words() {
        assert_eq!(progress_line("progress = 50%"), None);
    }
}
