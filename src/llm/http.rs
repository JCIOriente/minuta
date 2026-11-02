//! What the HTTP backends share.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;
use ureq::config::ConfigBuilder;
use ureq::typestate::AgentScope;

use crate::error::{Error, Result};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// A response whose status has not been judged yet.
pub(super) type Response = ureq::http::Response<ureq::Body>;

pub(super) fn agent(read_timeout: Duration) -> ConfigBuilder<AgentScope> {
    ureq::Agent::config_builder()
        .proxy(None)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_per_read(Some(read_timeout))
        .timeout_per_write(Some(CONNECT_TIMEOUT))
}

pub(super) fn base(url: &str) -> &str {
    url.trim_end_matches('/')
}

/// Why a response body could not be used.
pub(super) enum BodyError {
    /// Reading the body failed (I/O, timeout).
    Read(String),
    /// The body was read but is not JSON.
    NotJson(String),
}

impl BodyError {
    pub(super) fn is_transient(&self) -> bool {
        matches!(self, BodyError::Read(_))
    }

    pub(super) fn message(&self) -> &str {
        match self {
            BodyError::Read(m) | BodyError::NotJson(m) => m,
        }
    }
}

pub(super) fn read_json(mut response: Response) -> std::result::Result<Value, BodyError> {
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| BodyError::Read(e.to_string()))?;
    serde_json::from_str(&text).map_err(|e| BodyError::NotJson(e.to_string()))
}

/// At most 200 characters of untrusted text, without control characters, for messages.
pub(super) fn excerpt(text: &str) -> String {
    text.trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect()
}

pub(super) fn describe(e: &ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(code) => format!("HTTP status {code}"),
        other => other.to_string(),
    }
}

/// True when a failed connection is worth another attempt.
pub(super) fn is_transient(e: &ureq::Error) -> bool {
    matches!(
        e,
        ureq::Error::ConnectionFailed
            | ureq::Error::HostNotFound
            | ureq::Error::Io(_)
            | ureq::Error::Timeout(_)
    )
}

/// Runs `work` on a worker thread so an interrupt can abandon it. `died` is the error for a
/// worker that ended without an answer.
pub(super) fn on_worker<F>(
    work: F,
    interrupt: &AtomicBool,
    died: impl FnOnce() -> Error,
) -> Result<String>
where
    F: FnOnce() -> Result<String> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    loop {
        if interrupt.load(Ordering::SeqCst) {
            // The worker is abandoned; it ends at its read timeout.
            return Err(Error::Interrupted);
        }
        match rx.recv_timeout(POLL_INTERVAL) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(died()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_failure_is_transient() {
        assert!(is_transient(&ureq::Error::ConnectionFailed));
    }

    #[test]
    fn unknown_host_is_transient() {
        assert!(is_transient(&ureq::Error::HostNotFound));
    }

    #[test]
    fn io_failure_is_transient() {
        let reset = std::io::Error::from(std::io::ErrorKind::ConnectionReset);
        assert!(is_transient(&ureq::Error::Io(reset)));
    }

    #[test]
    fn timeout_is_transient() {
        assert!(is_transient(&ureq::Error::Timeout(ureq::Timeout::Global)));
    }

    #[test]
    fn bad_uri_is_not_transient() {
        assert!(!is_transient(&ureq::Error::BadUri("x".into())));
    }

    #[test]
    fn too_many_redirects_is_not_transient() {
        assert!(!is_transient(&ureq::Error::TooManyRedirects));
    }
}
