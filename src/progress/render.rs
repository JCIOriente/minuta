//! Spinner text: template events, and tool lines rendered as percents or sanitized.

use crate::llm::chunk::Event;
use crate::tools::{progress_line, ProgressLine};

/// The spinner message for a template event.
pub(super) fn event_text(event: Event) -> String {
    match event {
        Event::Chunk { done, total } => format!("chunk {done}/{total}"),
        Event::Consolidating => "consolidating".to_string(),
        Event::Merge { round, done, total } => {
            format!("consolidating (round {round}: {done}/{total})")
        }
        Event::Repair { attempt, total } => format!("repair {attempt}/{total}"),
    }
}

/// The spinner text for a tool line, `None` for a blank one: whisper's progress as `23%`,
/// an ffmpeg stats line as `45%  00:27/01:00` when the media length is known, and any
/// other line as it is, with control characters replaced.
pub(super) fn render_line(line: &str, media_seconds: Option<f64>) -> Option<String> {
    let line = latest_update(line)?;
    let text = match (progress_line(line), media_seconds) {
        (Some(ProgressLine::Whisper { percent }), _) => format!("{percent}%"),
        (Some(ProgressLine::FfmpegStats { time: Some(time) }), Some(total)) => {
            ffmpeg_percent(time, total)
        }
        _ => sanitize(line),
    };
    Some(text)
}

/// `45%  00:27/01:00` for `time` seconds done of `total`; past the end counts as the end.
/// Hours are shown when `total` reaches one: `48%  0:30:05/1:02:05`.
fn ffmpeg_percent(time: f64, total: f64) -> String {
    let done = time.min(total);
    // `as` saturates and `done / total` is within 0..=1, so this is 0..=100.
    let percent = (done / total * 100.0).floor() as u32;
    let with_hours = total >= 3600.0;
    format!(
        "{percent}%  {}/{}",
        clock(done, with_hours),
        clock(total, with_hours)
    )
}

/// `00:27` or, `with_hours`, `0:00:27`; fractions of a second are dropped.
fn clock(seconds: f64, with_hours: bool) -> String {
    let whole = seconds as u64;
    let (hours, minutes, secs) = (whole / 3600, whole / 60 % 60, whole % 60);
    if with_hours {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes:02}:{secs:02}")
    }
}

/// Tool output is untrusted: every control character becomes `?`, so it cannot move the
/// cursor or rewrite the terminal.
pub(super) fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// A tool line without surrounding whitespace; `None` when nothing is left. The tools
/// module already splits lines at `\r`.
fn latest_update(line: &str) -> Option<&str> {
    Some(line.trim()).filter(|line| !line.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_events() {
        assert_eq!(event_text(Event::Chunk { done: 2, total: 5 }), "chunk 2/5");
        assert_eq!(event_text(Event::Consolidating), "consolidating");
    }

    #[test]
    fn event_text_merge() {
        assert_eq!(
            event_text(Event::Merge {
                round: 1,
                done: 2,
                total: 3
            }),
            "consolidating (round 1: 2/3)"
        );
    }

    #[test]
    fn repair_event_text() {
        assert_eq!(
            event_text(Event::Repair {
                attempt: 1,
                total: 2
            }),
            "repair 1/2"
        );
    }

    #[test]
    fn latest_update_trims() {
        assert_eq!(
            latest_update("  [00:03.500] hola "),
            Some("[00:03.500] hola")
        );
    }

    #[test]
    fn latest_update_skips_blank_lines() {
        assert_eq!(latest_update(" \t "), None);
    }

    #[test]
    fn latest_update_skips_empty_lines() {
        assert_eq!(latest_update(""), None);
    }

    /// Real ffmpeg 9.0.2 stats of a 60 s re-encoding join, trimmed.
    const FFMPEG_JOIN_STATS: &str = include_str!("../../tests/fixtures/ffmpeg/join-stats.stderr");
    /// Real whisper-cli 1.9.5 `-pp` stderr for a 110 s clip with the tiny model, trimmed.
    const WHISPER_PP: &str = include_str!("../../tests/fixtures/whisper/pp.stderr");

    #[test]
    fn ffmpeg_stats_line_to_percent_normal() {
        assert_eq!(
            render_line(
                "frame=  662 fps=0.0 q=28.0 size=     256KiB time=00:00:27.40 \
                 bitrate= 123.4kbits/s speed=33.7x elapsed=0:00:00.50    ",
                Some(60.0)
            ),
            Some("45%  00:27/01:00".to_string())
        );
    }

    #[test]
    fn ffmpeg_stats_line_to_percent_audio_only() {
        assert_eq!(
            render_line(
                "size=     938KiB time=00:00:30.00 bitrate= 256.0kbits/s \
                 speed=2.35e+03x elapsed=0:00:00.01",
                Some(60.0)
            ),
            Some("50%  00:30/01:00".to_string())
        );
    }

    #[test]
    fn ffmpeg_stats_line_to_percent_time_not_available() {
        let line = "frame=    0 fps=0.0 q=0.0 size=       0KiB time=N/A bitrate=N/A speed=N/A";
        assert_eq!(render_line(line, Some(60.0)), Some(line.to_string()));
    }

    #[test]
    fn ffmpeg_stats_line_to_percent_unknown_duration() {
        let line = "frame=  662 fps=0.0 q=28.0 size=     256KiB time=00:00:27.40 speed=33.7x";
        assert_eq!(render_line(line, None), Some(line.to_string()));
    }

    #[test]
    fn ffmpeg_stats_line_to_percent_past_duration_clamps() {
        assert_eq!(
            render_line(
                "frame= 1525 fps=906 q=-1.0 Lsize=    1116KiB time=00:01:01.00 speed=36.2x",
                Some(60.0)
            ),
            Some("100%  01:00/01:00".to_string())
        );
    }

    #[test]
    fn ffmpeg_stats_line_to_percent_negative_time() {
        assert_eq!(
            render_line(
                "frame=    0 fps=0.0 q=0.0 size=       0KiB time=-00:00:00.02 speed=N/A",
                Some(60.0)
            ),
            Some("0%  00:00/01:00".to_string())
        );
    }

    #[test]
    fn ffmpeg_stats_line_to_percent_hours() {
        assert_eq!(
            render_line(
                "size=  4096KiB time=00:30:05.90 bitrate= 256.0kbits/s speed=40x",
                Some(3725.0)
            ),
            Some("48%  0:30:05/1:02:05".to_string())
        );
    }

    #[test]
    fn ffmpeg_stats_capture_to_percent() {
        let rendered: Vec<String> = FFMPEG_JOIN_STATS
            .split(['\r', '\n'])
            .filter_map(|line| render_line(line, Some(60.0)))
            .collect();
        assert_eq!(
            rendered,
            [
                "encoder         : Lavc63.1.102 aac",
                "28%  00:16/01:00",
                "53%  00:32/01:00",
                "89%  00:53/01:00",
                "[out#0/mp4 @ 0x79794106c0] video:558KiB audio:507KiB subtitle:0KiB other \
                 streams:0KiB global headers:0KiB muxing overhead: 4.741262%",
                "100%  01:00/01:00",
                "[libx264 @ 0x797945df80] frame I:6     Avg QP:10.53  size:  8473",
            ]
        );
    }

    #[test]
    fn whisper_progress_line_to_percent() {
        assert_eq!(
            render_line("whisper_print_progress_callback: progress =  23%", None),
            Some("23%".to_string())
        );
    }

    #[test]
    fn whisper_progress_line_to_percent_from_real_capture() {
        let rendered: Vec<String> = WHISPER_PP
            .lines()
            .filter_map(|line| render_line(line, None))
            .collect();
        assert_eq!(
            rendered,
            [
                "main: processing '/tmp/jfk10.wav' (1760000 samples, 110.0 sec), 4 threads, \
                 1 processors, 5 beams + best of 5, lang = en, task = transcribe, \
                 timestamps = 1 ...",
                "20%",
                "40%",
                "60%",
                "80%",
                "100%",
                "output_json: saving output to '/tmp/jfk10-out.json'",
                "whisper_print_timings:     load time =    33.40 ms",
            ]
        );
    }

    #[test]
    fn whisper_percent_ignores_media_seconds() {
        assert_eq!(
            render_line(
                "whisper_print_progress_callback: progress =  40%",
                Some(60.0)
            ),
            Some("40%".to_string())
        );
    }

    #[test]
    fn raw_tool_line_is_sanitized() {
        assert_eq!(
            render_line("\u{1b}[2Kerror:\tboom\u{7}\u{9b}", None),
            Some("?[2Kerror:?boom??".to_string())
        );
    }
}
