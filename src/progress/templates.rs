use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

use super::render::sanitize;
use super::Mode;

/// Shows the download of the template files of one `templates pull`. On an interactive
/// terminal a count bar (`done/total`, current template) is drawn on stderr; in plain mode
/// and in `--quiet` nothing is drawn. Only the files that will be downloaded are counted.
pub struct TemplatesReport {
    mode: Mode,
    total: u64,
    bar: Option<ProgressBar>,
}

impl TemplatesReport {
    pub fn new(mode: Mode, total: usize) -> TemplatesReport {
        TemplatesReport {
            mode,
            total: total as u64,
            bar: None,
        }
    }

    /// A fetch of template `name` starts. The first call creates the bar on an interactive
    /// terminal, when there is something to fetch.
    pub fn fetching(&mut self, name: &str) {
        if self.mode != Mode::Live || self.total == 0 {
            return;
        }
        let total = self.total;
        let bar = self.bar.get_or_insert_with(|| {
            let style = ProgressStyle::with_template("{prefix}  [{bar:20}]  {pos}/{len}  {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("█░░");
            let bar = ProgressBar::new(total)
                .with_style(style)
                .with_prefix("templates");
            bar.enable_steady_tick(Duration::from_millis(200));
            bar
        });
        bar.set_message(format!("fetching {}", sanitize(name)));
    }

    /// The fetch started last is done.
    pub fn fetched(&mut self) {
        if let Some(bar) = &self.bar {
            bar.inc(1);
        }
    }

    #[cfg(test)]
    pub(crate) fn total(&self) -> u64 {
        self.total
    }

    #[cfg(test)]
    pub(crate) fn bar_active(&self) -> bool {
        self.bar.is_some()
    }

    /// Removes the bar, so the lines that follow (results or an error) print cleanly.
    pub fn clear(&mut self) {
        if let Some(bar) = self.bar.take() {
            bar.finish_and_clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_templates_bar_counts_and_clears() {
        let mut report = TemplatesReport::new(Mode::Live, 2);
        assert_eq!(report.total, 2);
        report.fetching("alpha");
        assert_eq!(report.bar.as_ref().map(|b| b.length()), Some(Some(2)));
        report.fetched();
        assert_eq!(report.bar.as_ref().map(|b| b.position()), Some(1));
        report.clear();
        assert!(report.bar.is_none());
    }

    #[test]
    fn templates_bar_is_not_drawn_outside_live_mode_or_with_nothing_to_fetch() {
        for mode in [Mode::Plain, Mode::Quiet] {
            let mut report = TemplatesReport::new(mode, 3);
            report.fetching("alpha");
            report.fetched();
            assert!(report.bar.is_none());
        }
        let mut report = TemplatesReport::new(Mode::Live, 0);
        report.fetching("alpha");
        assert!(report.bar.is_none());
    }
}
