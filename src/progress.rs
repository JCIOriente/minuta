//! Progress on stderr: a step counter over pipeline steps and templates, per-step timing,
//! live tool output or template progress, and a summary. stdout is never written here.

use std::ffi::OsString;
use std::io::IsTerminal;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

use crate::job::StepName;
use crate::llm::chunk::Event;

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
}

impl Progress {
    pub fn new(mode: Mode) -> Progress {
        Progress {
            mode,
            total: StepName::ALL.len(),
            spinner: None,
        }
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
        if let (Some(spinner), Some(latest)) = (&self.spinner, latest_update(line)) {
            spinner.set_message(latest.to_string());
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

/// The last non-blank part of a tool line; ffmpeg separates its status updates with `\r`.
fn latest_update(line: &str) -> Option<&str> {
    line.rsplit('\r')
        .map(str::trim)
        .find(|part| !part.is_empty())
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
    fn latest_update_takes_the_last_carriage_return_part() {
        assert_eq!(
            latest_update("frame=1 time=00:00:01\rframe=2 time=00:00:02\r"),
            Some("frame=2 time=00:00:02")
        );
        assert_eq!(
            latest_update("  [00:03.500] hola "),
            Some("[00:03.500] hola")
        );
        assert_eq!(latest_update(" \r "), None);
    }
}
