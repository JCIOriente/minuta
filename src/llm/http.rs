//! What the HTTP backends share.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::Value;

use crate::error::{Error, Result};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

pub(super) fn agent(read_timeout: Duration) -> ureq::AgentBuilder {
    ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(read_timeout)
        .timeout_write(CONNECT_TIMEOUT)
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

pub(super) fn read_json(response: ureq::Response) -> std::result::Result<Value, BodyError> {
    let text = response
        .into_string()
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
        ureq::Error::Status(code, _) => format!("HTTP status {code}"),
        ureq::Error::Transport(t) => t.to_string(),
    }
}

/// True when a failed connection is worth another attempt.
pub(super) fn transport_is_transient(t: &ureq::Transport) -> bool {
    matches!(
        t.kind(),
        ureq::ErrorKind::ConnectionFailed
            | ureq::ErrorKind::Dns
            | ureq::ErrorKind::Io
            | ureq::ErrorKind::ProxyConnect
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
