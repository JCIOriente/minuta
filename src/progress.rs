//! Progress on stderr: a step counter over pipeline steps and templates, per-step timing,
//! live tool output or template progress, and a summary. stdout is never written here.

use std::ffi::OsString;
use std::io::IsTerminal;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

use crate::job::StepName;
use crate::llm::chunk::Event;
use crate::tools::{progress_line, ProgressLine};

/// How much progress to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Warnings only.
    Quiet,
    /// One plain line per step.
    Plain,
    /// Plain lines plus a spinner showing the running tool's latest output line.
    Live,
}

impl Mode {
    /// `--quiet` wins; `NO_COLOR` (set and non-empty) or a stderr that is not a terminal
    /// turns off the spinner.
    pub fn choose(quiet: bool, no_color: Option<OsString>, stderr_is_terminal: bool) -> Mode {
        if quiet {
            Mode::Quiet
        } else if no_color.is_some_and(|v| !v.is_empty()) || !stderr_is_terminal {
            Mode::Plain
        } else {
            Mode::Live
        }
    }

    /// [`Mode::choose`] for this process.
    pub fn detect(quiet: bool) -> Mode {
        Mode::choose(
            quiet,
            std::env::var_os("NO_COLOR"),
            std::io::stderr().is_terminal(),
        )
    }
}

/// Reports the steps and templates of one run. A label is a step or template name.
pub struct Progress {
    mode: Mode,
    total: usize,
    spinner: Option<ProgressBar>,
    /// Length in seconds of the media the running step processes, for ffmpeg's percent.
    media_seconds: Option<f64>,
}

impl Progress {
    pub fn new(mode: Mode) -> Progress {
        Progress {
            mode,
            total: StepName::ALL.len(),
            spinner: None,
            media_seconds: None,
        }
    }

    /// The running step processes `seconds` of media, so ffmpeg stats lines can show a
    /// percent. [`Progress::start`] forgets it; `None` shows the raw stats line.
    pub fn set_media_seconds(&mut self, seconds: Option<f64>) {
        self.media_seconds = seconds;
    }

    /// The number of steps plus templates in the run (the pipeline steps by default).
    pub fn set_total(&mut self, total: usize) {
        self.total = total;
    }

    fn print(&self, text: &str) {
        if self.mode != Mode::Quiet {
            eprintln!("{text}");
        }
    }

    /// An informational message, hidden by `--quiet`.
    pub fn note(&mut self, text: &str) {
        self.print(&format!("note: {text}"));
    }

    /// A note that is shown even with `--quiet`, such as data leaving the machine.
    pub fn notice(&mut self, text: &str) {
        eprintln!("note: {text}");
    }

    /// A warning, shown even with `--quiet`.
    pub fn warn(&mut self, text: &str) {
        eprintln!("warning: {text}");
    }

    /// `label` (1-based `index`) is reused from an earlier run.
    pub fn cached(&mut self, index: usize, label: &str) {
        self.print(&step_line(index, self.total, label, "cached"));
    }

    /// Step `label` starts running; the spinner shows the tool's latest output line.
    pub fn start(&mut self, index: usize, label: &str) {
        self.spin(
            index,
            label,
            "{spinner:.cyan} {prefix:.bold} {wide_msg:.dim}",
        );
    }

    /// Template `label` starts running; the spinner shows its chunk progress and the
    /// elapsed time.
    pub fn start_template(&mut self, index: usize, label: &str) {
        self.spin(
            index,
            label,
            "{spinner:.cyan} {prefix:.bold} {msg:.dim}  {elapsed:.dim}",
        );
    }

    fn spin(&mut self, index: usize, label: &str, template: &str) {
        self.media_seconds = None;
        if self.mode != Mode::Live {
            return;
        }
        let style = ProgressStyle::with_template(template).expect("valid template");
        let spinner = ProgressBar::new_spinner()
            .with_style(style)
            .with_prefix(format!("[{index}/{}] {label}", self.total));
        spinner.enable_steady_tick(Duration::from_millis(100));
        self.spinner = Some(spinner);
    }

    /// Progress of the running template.
    pub fn event(&mut self, event: Event) {
        if let Some(spinner) = &self.spinner {
            spinner.set_message(event_text(event));
        }
    }

    /// A line of output from the running tool.
    pub fn line(&mut self, line: &str) {
        if let (Some(spinner), Some(text)) = (&self.spinner, render_line(line, self.media_seconds))
        {
            spinner.set_message(text);
        }
    }

    /// `label` finished after `seconds`; `detail` says what it did, when worth saying.
    pub fn done(&mut self, index: usize, label: &str, detail: Option<&str>, seconds: f64) {
        self.clear();
        let time = format_duration(seconds);
        let text = match detail {
            Some(detail) => format!("{detail}  {time}"),
            None => time,
        };
        self.print(&step_line(index, self.total, label, &text));
    }

    /// `label` stopped early; the caller reports the error itself.
    pub fn failed(&mut self, index: usize, label: &str, interrupted: bool) {
        self.clear();
        let what = if interrupted { "interrupted" } else { "failed" };
        self.print(&step_line(index, self.total, label, what));
    }

    /// The final line of a run.
    pub fn summary(&mut self, ran: usize, cached: usize, seconds: f64) {
        self.print(&format!(
            "finished in {} ({ran} run, {cached} cached)",
            format_duration(seconds)
        ));
    }

    fn clear(&mut self) {
        if let Some(spinner) = self.spinner.take() {
            spinner.finish_and_clear();
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.clear();
    }
}

/// `[2/5] join        stream copy  12.4s`: labels are padded so the text lines up.
fn step_line(index: usize, total: usize, label: &str, text: &str) -> String {
    format!("[{index}/{total}] {label:<12}{text}")
}

/// What a finished template did: `ollama:gemma4:26b  3 calls`.
pub fn template_detail(backend: &str, calls: usize) -> String {
    let plural = if calls == 1 { "" } else { "s" };
    format!("{backend}  {calls} call{plural}")
}

/// The spinner message for a template event.
fn event_text(event: Event) -> String {
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
fn render_line(line: &str, media_seconds: Option<f64>) -> Option<String> {
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
fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// A tool line without surrounding whitespace; `None` when nothing is left. The tools
/// module already splits lines at `\r`.
fn latest_update(line: &str) -> Option<&str> {
    Some(line.trim()).filter(|line| !line.is_empty())
}

/// `0.3s`, `12.4s`, `2m31s`, `1h05m`.
pub fn format_duration(seconds: f64) -> String {
    if seconds < 59.95 {
        return format!("{seconds:.1}s");
    }
    let whole = seconds.round() as u64;
    let (hours, minutes, secs) = (whole / 3600, whole / 60 % 60, whole % 60);
    if hours > 0 {
        format!("{hours}h{minutes:02}m")
    } else {
        format!("{minutes}m{secs:02}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_wins() {
        assert_eq!(Mode::choose(true, None, true), Mode::Quiet);
        assert_eq!(Mode::choose(true, Some("1".into()), false), Mode::Quiet);
    }

    #[test]
    fn no_color_or_non_terminal_is_plain() {
        assert_eq!(Mode::choose(false, Some("1".into()), true), Mode::Plain);
        assert_eq!(Mode::choose(false, None, false), Mode::Plain);
        // An empty NO_COLOR does not count.
        assert_eq!(Mode::choose(false, Some("".into()), true), Mode::Live);
        assert_eq!(Mode::choose(false, None, true), Mode::Live);
    }

    #[test]
    fn durations() {
        assert_eq!(format_duration(0.04), "0.0s");
        assert_eq!(format_duration(0.3), "0.3s");
        assert_eq!(format_duration(12.44), "12.4s");
        assert_eq!(format_duration(59.97), "1m00s");
        assert_eq!(format_duration(151.0), "2m31s");
        assert_eq!(format_duration(3900.0), "1h05m");
    }

    #[test]
    fn step_lines_line_up() {
        assert_eq!(step_line(1, 5, "probe", "0.3s"), "[1/5] probe       0.3s");
        assert_eq!(
            step_line(4, 5, "transcribe", "cached"),
            "[4/5] transcribe  cached"
        );
    }

    #[test]
    fn template_line_shows_backend_calls_and_time() {
        let text = format!("{}  {}", template_detail("ollama:gemma4:26b", 1), "48.2s");
        assert_eq!(
            step_line(6, 6, "summary", &text),
            "[6/6] summary     ollama:gemma4:26b  1 call  48.2s"
        );
        assert_eq!(template_detail("claude", 3), "claude  3 calls");
    }

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
    fn total_defaults_to_the_pipeline_steps() {
        let mut progress = Progress::new(Mode::Quiet);
        assert_eq!(progress.total, 5);
        progress.set_total(7);
        assert_eq!(progress.total, 7);
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
    const FFMPEG_JOIN_STATS: &str = include_str!("../tests/fixtures/ffmpeg/join-stats.stderr");
    /// Real whisper-cli 1.9.5 `-pp` stderr for a 110 s clip with the tiny model, trimmed.
    const WHISPER_PP: &str = include_str!("../tests/fixtures/whisper/pp.stderr");

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

    #[test]
    fn spinner_shows_ffmpeg_percent_of_media_seconds() {
        let mut progress = Progress::new(Mode::Live);
        progress.start(2, "join");
        progress.set_media_seconds(Some(60.0));
        progress.line("frame=  662 fps=0.0 q=28.0 size=  256KiB time=00:00:27.40 speed=33.7x");
        let message = progress.spinner.as_ref().map(ProgressBar::message);
        assert_eq!(message.as_deref(), Some("45%  00:27/01:00"));
    }

    #[test]
    fn start_forgets_media_seconds() {
        let mut progress = Progress::new(Mode::Live);
        progress.start(2, "join");
        progress.set_media_seconds(Some(60.0));
        progress.start(3, "audio");
        let line = "size=  256KiB time=00:00:27.40 bitrate= 256.0kbits/s speed=33.7x";
        progress.line(line);
        let message = progress.spinner.as_ref().map(ProgressBar::message);
        assert_eq!(message.as_deref(), Some(line));
    }
}
