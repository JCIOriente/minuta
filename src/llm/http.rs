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
/// How many redirects a client that follows them takes before it gives up.
pub(crate) const MAX_REDIRECTS: u32 = 4;

/// A response whose status has not been judged yet.
pub(super) type Response = ureq::http::Response<ureq::Body>;

pub(super) fn agent(read_timeout: Duration) -> ConfigBuilder<AgentScope> {
    ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(MAX_REDIRECTS)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_per_read(Some(read_timeout))
        .timeout_per_write(Some(CONNECT_TIMEOUT))
}

/// `url` as ureq can send it. ureq parses into `http::Uri`, which refuses what `url` normalizes,
/// such as a non-ASCII host or a space in the path. The check must come first: ureq swaps a port
/// it cannot parse for the scheme's default, and sends plain HTTP to a `socks5://` address.
pub(crate) fn request_url(url: &str) -> std::result::Result<String, String> {
    let parsed = url::Url::parse(url).map_err(|e| e.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("not an http or https URL".to_string());
    }
    Ok(parsed.into())
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
    fn request_url_encodes_a_non_ascii_host() {
        assert_eq!(
            request_url("http://b\u{fc}cher.example:8080/v1").as_deref(),
            Ok("http://xn--bcher-kva.example:8080/v1")
        );
    }

    #[test]
    fn request_url_encodes_a_space_in_the_path() {
        assert_eq!(
            request_url("http://host/a b").as_deref(),
            Ok("http://host/a%20b")
        );
    }

    #[test]
    fn request_url_adds_the_missing_slashes() {
        assert_eq!(
            request_url("http:host:1/v1").as_deref(),
            Ok("http://host:1/v1")
        );
    }

    #[test]
    fn request_url_refuses_what_it_cannot_parse() {
        assert_eq!(
            request_url("no-scheme/x"),
            Err("relative URL without a base".to_string())
        );
    }

    #[test]
    fn request_url_refuses_an_invalid_port() {
        assert_eq!(
            request_url("http://host:abc/v1"),
            Err("invalid port number".to_string())
        );
    }

    fn refused_scheme() -> std::result::Result<String, String> {
        Err("not an http or https URL".to_string())
    }

    #[test]
    fn request_url_refuses_socks() {
        assert_eq!(request_url("socks://host:1080"), refused_scheme());
    }

    #[test]
    fn request_url_refuses_socks4() {
        assert_eq!(request_url("socks4://host:1080"), refused_scheme());
    }

    #[test]
    fn request_url_refuses_socks4a() {
        assert_eq!(request_url("socks4a://host:1080"), refused_scheme());
    }

    #[test]
    fn request_url_refuses_socks5() {
        assert_eq!(request_url("socks5://host:1080"), refused_scheme());
    }

    #[test]
    fn request_url_refuses_socks5h() {
        assert_eq!(request_url("socks5h://host:1080"), refused_scheme());
    }

    #[test]
    fn request_url_keeps_https() {
        assert_eq!(
            request_url("https://host/v1").as_deref(),
            Ok("https://host/v1")
        );
    }

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
