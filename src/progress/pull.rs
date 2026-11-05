//! Model download progress: size formatting, plain start and finish lines, the byte bar.

use std::path::Path;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

use crate::models::Pulled;

use super::Mode;

/// A model size as `141.1 MiB` or `2.9 GiB`.
pub fn human_size(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    let mib = bytes as f64 / MIB;
    if mib < 1024.0 {
        format!("{mib:.1} MiB")
    } else {
        format!("{:.1} GiB", mib / 1024.0)
    }
}

/// The plain start line of a model download.
pub fn pull_start_line(name: &str, total: u64, dir: &Path) -> String {
    format!(
        "pulling {name} ({}) into {}",
        human_size(total),
        dir.display()
    )
}

/// The plain finish line of a model download.
pub fn pull_finish_line(name: &str, outcome: Pulled) -> String {
    match outcome {
        Pulled::AlreadyPresent => format!("{name} is already present"),
        Pulled::Downloaded => format!("{name} downloaded and verified"),
    }
}

/// Shows one model download. On an interactive terminal a byte bar (percent, rate, ETA)
/// appears once bytes arrive; otherwise one start line and one finish line. An already
/// present model prints its finish line in both modes. `--quiet` shows nothing.
pub struct PullReport {
    mode: Mode,
    name: String,
    total: u64,
    bar: Option<ProgressBar>,
    /// Every printed line, in order, so tests can see what was shown.
    #[cfg(test)]
    pub(crate) shown: Vec<String>,
}

impl PullReport {
    pub fn new(mode: Mode) -> PullReport {
        PullReport {
            mode,
            name: String::new(),
            total: 0,
            bar: None,
            #[cfg(test)]
            shown: Vec::new(),
        }
    }

    fn print(&mut self, text: String) {
        if self.mode != Mode::Quiet {
            eprintln!("{text}");
            #[cfg(test)]
            self.shown.push(text);
        }
    }

    /// Starts the download of `total` bytes into `dir`. Plain mode prints the start line now.
    pub fn start(&mut self, name: &str, total: u64, dir: &Path) {
        self.name = name.to_string();
        self.total = total;
        if self.mode == Mode::Plain {
            self.print(pull_start_line(name, total, dir));
        }
    }

    /// The running byte count, from the download's callback. The first call creates the bar
    /// on an interactive terminal.
    pub fn progress(&mut self, done: u64) {
        if self.mode != Mode::Live {
            return;
        }
        let total = self.total;
        let name = self.name.clone();
        let bar = self.bar.get_or_insert_with(|| {
            let style = ProgressStyle::with_template(
                "{msg} [{bar:30}] {percent:>3}% {bytes}/{total_bytes} {bytes_per_sec} eta {eta}",
            )
            .unwrap_or_else(|_| ProgressStyle::default_bar());
            let bar = ProgressBar::new(total).with_style(style).with_message(name);
            bar.enable_steady_tick(Duration::from_millis(200));
            bar
        });
        bar.set_position(done);
    }

    /// The download is verified and in place. A bar that was drawn is cleared; otherwise
    /// (plain mode, or a model that was already present) the finish line prints.
    pub fn finish(&mut self, name: &str, outcome: Pulled) {
        match self.bar.take() {
            Some(bar) => bar.finish_and_clear(),
            None => self.print(pull_finish_line(name, outcome)),
        }
    }

    /// The download failed: clears the bar so the error prints on a clean line.
    pub fn abandon(&mut self) {
        if let Some(bar) = self.bar.take() {
            bar.finish_and_clear();
        }
    }
}

#[cfg(test)]
mod pull_report_tests {
    use super::*;

    #[test]
    fn pull_start_line_names_model_size_and_dir() {
        assert_eq!(
            pull_start_line("base", 147_951_465, Path::new("/m")),
            "pulling base (141.1 MiB) into /m"
        );
    }

    #[test]
    fn pull_finish_line_when_downloaded() {
        assert_eq!(
            pull_finish_line("base", Pulled::Downloaded),
            "base downloaded and verified"
        );
    }

    #[test]
    fn plain_pull_prints_start_then_finish() {
        let mut report = PullReport::new(Mode::Plain);
        report.start("base", 147_951_465, Path::new("/m"));
        report.progress(10);
        report.finish("base", Pulled::Downloaded);
        assert_eq!(
            report.shown,
            [
                "pulling base (141.1 MiB) into /m".to_string(),
                "base downloaded and verified".to_string(),
            ]
        );
    }

    #[test]
    fn quiet_pull_prints_nothing() {
        let mut report = PullReport::new(Mode::Quiet);
        report.start("base", 147_951_465, Path::new("/m"));
        report.finish("base", Pulled::Downloaded);
        assert!(report.shown.is_empty());
    }

    #[test]
    fn live_download_draws_a_bar_not_lines() {
        let mut report = PullReport::new(Mode::Live);
        report.start("base", 147_951_465, Path::new("/m"));
        report.progress(10);
        report.finish("base", Pulled::Downloaded);
        assert!(report.shown.is_empty());
    }

    #[test]
    fn live_already_present_prints_its_finish_line() {
        let mut report = PullReport::new(Mode::Live);
        report.start("base", 147_951_465, Path::new("/m"));
        report.finish("base", Pulled::AlreadyPresent);
        assert_eq!(report.shown, ["base is already present".to_string()]);
    }

    #[test]
    fn abandon_clears_the_live_bar() {
        let mut report = PullReport::new(Mode::Live);
        report.start("base", 147_951_465, Path::new("/m"));
        report.progress(10);
        assert!(report.bar.is_some());
        report.abandon();
        assert!(report.bar.is_none());
    }
}
