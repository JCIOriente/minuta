//! Helpers shared by the unit tests. The two fakes use only `std` and `serde_json`, never
//! `crate::`, so the test crates in `tests/` include the same files with `#[path]`.

pub(crate) mod fake_http;
pub(crate) mod fake_ollama;

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{Error, Result};

/// The committed test fixtures, independent of the working directory.
pub(crate) const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

/// How often [`retry_busy`] runs the operation before it returns the busy error.
const BUSY_ATTEMPTS: u32 = 20;
/// The pause between two attempts, long enough for the other thread's fork to `exec`.
const BUSY_PAUSE: Duration = Duration::from_millis(25);

/// The fixture at `rel` under [`FIXTURES`].
pub(crate) fn fixture(rel: &str) -> PathBuf {
    Path::new(FIXTURES).join(rel)
}

/// Runs `op`, retrying while it fails with ETXTBSY ("Text file busy"): on Linux, `exec` of a
/// freshly written script fails while a fork in another test thread still holds it open for
/// writing. Covers the error as `Error::Io` and as the hint of an `Error::MissingTools` (tool
/// discovery reports a failed version probe that way). Any other result returns at once;
/// after [`BUSY_ATTEMPTS`] busy failures, the last one is returned.
pub(crate) fn retry_busy<T>(mut op: impl FnMut() -> Result<T>) -> Result<T> {
    let mut attempts = 0;
    loop {
        let res = op();
        attempts += 1;
        if !is_busy(&res) || attempts >= BUSY_ATTEMPTS {
            return res;
        }
        std::thread::sleep(BUSY_PAUSE);
    }
}

fn is_busy<T>(res: &Result<T>) -> bool {
    const BUSY: &str = "Text file busy";
    match res {
        Err(Error::Io(e)) => e.to_string().contains(BUSY),
        Err(Error::MissingTools(missing)) => missing.iter().any(|m| m.hint.contains(BUSY)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io;

    use super::*;
    use crate::error::{Error, Missing};

    /// What Linux reports when `exec` hits a file another thread's fork holds open.
    const BUSY: &str = "Text file busy (os error 26)";

    fn busy_io() -> Error {
        Error::Io(io::Error::other(BUSY))
    }

    fn missing(name: &str, hint: &str) -> Missing {
        Missing {
            name: name.into(),
            hint: hint.into(),
        }
    }

    #[test]
    fn retry_busy_retries_a_busy_io_error() {
        let calls = Cell::new(0);
        let res = retry_busy(|| {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                Err(busy_io())
            } else {
                Ok("done")
            }
        });
        assert_eq!(res.unwrap(), "done");
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn retry_busy_retries_a_busy_missing_tools_hint() {
        let calls = Cell::new(0);
        let res = retry_busy(|| {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                // The busy hint is on the second tool, so every hint is checked.
                Err(Error::MissingTools(vec![
                    missing("ffmpeg", "install ffmpeg"),
                    missing("whisper-cli", &format!("could not run whisper-cli: {BUSY}")),
                ]))
            } else {
                Ok("done")
            }
        });
        assert_eq!(res.unwrap(), "done");
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn retry_busy_returns_other_errors_at_once() {
        let calls = Cell::new(0);
        let res: Result<()> = retry_busy(|| {
            calls.set(calls.get() + 1);
            Err(Error::Io(io::Error::other(
                "Permission denied (os error 13)",
            )))
        });
        assert_eq!(
            res.unwrap_err().to_string(),
            "I/O error: Permission denied (os error 13)"
        );
        assert_eq!(calls.get(), 1);

        let calls = Cell::new(0);
        let res: Result<()> = retry_busy(|| {
            calls.set(calls.get() + 1);
            Err(Error::MissingTools(vec![
                missing("ffmpeg", "install ffmpeg"),
                missing("whisper-cli", "install whisper-cpp"),
            ]))
        });
        assert_eq!(
            res.unwrap_err().to_string(),
            "missing required tools: ffmpeg, whisper-cli"
        );
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn retry_busy_gives_up_after_its_attempts() {
        let calls = Cell::new(0);
        let res: Result<()> = retry_busy(|| {
            calls.set(calls.get() + 1);
            Err(busy_io())
        });
        assert_eq!(res.unwrap_err().to_string(), format!("I/O error: {BUSY}"));
        assert_eq!(calls.get(), 20);
    }

    #[test]
    fn fixture_points_into_tests_fixtures() {
        let path = fixture("bin/whisper-cli");
        assert_eq!(
            path,
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/whisper-cli")
        );
        assert!(path.is_file(), "{}", path.display());
    }
}
