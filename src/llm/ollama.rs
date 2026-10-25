//! Ollama HTTP backend.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{json, Value};

use super::Backend;
use crate::error::{Error, Missing, Result};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);
const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// What `/api/show` says about a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OllamaInfo {
    pub context_length: Option<usize>,
    pub thinking: bool,
}

fn agent(read_timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(read_timeout)
        .timeout_write(CONNECT_TIMEOUT)
        .build()
}

fn base(url: &str) -> &str {
    url.trim_end_matches('/')
}

/// Verifies the server is reachable and `model` is pulled, then reads its limits.
pub fn check(url: &str, model: &str) -> std::result::Result<OllamaInfo, Missing> {
    let base = base(url);
    let agent = agent(CHECK_TIMEOUT);
    let server_down = |why: String| Missing {
        name: "ollama".into(),
        hint: format!("start Ollama (`ollama serve`); {why} at {base}"),
    };
    let model_problem = |why: String| Missing {
        name: format!("ollama:{model}"),
        hint: why,
    };

    let tags = agent
        .get(&format!("{base}/api/tags"))
        .call()
        .map_err(|e| server_down(describe(&e)))?;
    let tags = read_json(tags)
        .map_err(|e| server_down(format!("unreadable /api/tags response ({})", e.0)))?;
    let Some(models) = tags["models"].as_array() else {
        return Err(server_down(format!(
            "unexpected /api/tags response ({tags})"
        )));
    };
    let pulled = models
        .iter()
        .filter_map(|m| m["name"].as_str())
        .any(|name| name_matches(name, model));
    if !pulled {
        return Err(model_problem(format!("ollama pull {model}")));
    }

    let shown = agent
        .post(&format!("{base}/api/show"))
        .set("Content-Type", "application/json")
        .send_string(&json!({ "model": model }).to_string())
        .map_err(|e| model_problem(format!("ollama show {model} failed: {}", describe(&e))))?;
    let shown = read_json(shown)
        .map_err(|e| model_problem(format!("unreadable /api/show response ({})", e.0)))?;
    let context_length = shown["model_info"].as_object().and_then(|info| {
        info.iter()
            .find(|(key, _)| key.ends_with(".context_length"))
            .and_then(|(_, v)| v.as_u64())
            .and_then(|n| usize::try_from(n).ok())
    });
    let thinking = shown["capabilities"]
        .as_array()
        .is_some_and(|caps| caps.iter().any(|c| c == "thinking"));
    Ok(OllamaInfo {
        context_length,
        thinking,
    })
}

/// A bare name (no tag) matches `name:latest`.
fn name_matches(listed: &str, wanted: &str) -> bool {
    listed == wanted || (!wanted.contains(':') && listed == format!("{wanted}:latest"))
}

/// Reads a JSON body. The error carries whether it is transient: a failed body read is,
/// a body that is not JSON is not.
fn read_json(response: ureq::Response) -> std::result::Result<Value, (String, bool)> {
    let text = response.into_string().map_err(|e| (e.to_string(), true))?;
    serde_json::from_str(&text).map_err(|e| (e.to_string(), false))
}

fn describe(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, _) => format!("HTTP status {code}"),
        ureq::Error::Transport(t) => t.to_string(),
    }
}

#[derive(Clone)]
pub struct Ollama {
    id: String,
    model: String,
    url: String,
    num_ctx: usize,
    context_tokens: usize,
    thinking: bool,
    timeout: Duration,
}

impl Ollama {
    pub fn new(url: &str, model: &str, num_ctx: usize, info: &OllamaInfo) -> Ollama {
        Ollama {
            id: format!("ollama:{model}"),
            model: model.to_string(),
            url: base(url).to_string(),
            num_ctx,
            context_tokens: info.context_length.map_or(num_ctx, |n| n.min(num_ctx)),
            thinking: info.thinking,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Read timeout for a chat request (default 10 minutes).
    pub fn with_timeout(mut self, timeout: Duration) -> Ollama {
        self.timeout = timeout;
        self
    }

    fn error(&self, message: String, transient: bool) -> Error {
        Error::Llm {
            backend: self.id.clone(),
            message,
            transient,
        }
    }

    fn body(&self, system: &str, user: &str) -> Value {
        let mut body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "stream": false,
            "options": {"num_ctx": self.num_ctx},
        });
        if self.thinking {
            body["think"] = json!(false);
        }
        body
    }

    /// One blocking round trip; runs on a worker thread.
    fn request(&self, body: Value) -> Result<String> {
        let response = agent(self.timeout)
            .post(&format!("{}/api/chat", self.url))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string());
        let response = match response {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => return Err(self.status_error(code, r)),
            Err(ureq::Error::Transport(t)) => {
                let transient = matches!(
                    t.kind(),
                    ureq::ErrorKind::ConnectionFailed
                        | ureq::ErrorKind::Dns
                        | ureq::ErrorKind::Io
                        | ureq::ErrorKind::ProxyConnect
                );
                return Err(self.error(format!("request failed: {t}"), transient));
            }
        };
        let parsed = read_json(response)
            .map_err(|(e, transient)| self.error(format!("unreadable response: {e}"), transient))?;
        if parsed["done_reason"] == "length" {
            return Err(self.error(
                "response cut off at the context limit; raise ollama.num_ctx".into(),
                false,
            ));
        }
        parsed["message"]["content"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| self.error("response has no message content".into(), false))
    }

    fn status_error(&self, code: u16, response: ureq::Response) -> Error {
        let detail = match response.into_string() {
            Ok(text) => {
                let from_json = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v["error"].as_str().map(str::to_string));
                let shown = from_json.unwrap_or_else(|| text.trim().chars().take(200).collect());
                if shown.is_empty() {
                    String::new()
                } else {
                    format!(": {shown}")
                }
            }
            Err(_) => String::new(),
        };
        match code {
            404 => self.error(
                format!(
                    "HTTP 404{detail}; the model may not be pulled (ollama pull {})",
                    self.model
                ),
                false,
            ),
            429 | 500..=599 => self.error(format!("HTTP {code}{detail}"), true),
            _ => self.error(format!("HTTP {code}{detail}"), false),
        }
    }
}

impl Backend for Ollama {
    fn id(&self) -> &str {
        &self.id
    }

    fn context_tokens(&self) -> usize {
        self.context_tokens
    }

    fn complete(&self, system: &str, user: &str, interrupt: &AtomicBool) -> Result<String> {
        let body = self.body(system, user);
        let worker = self.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(worker.request(body));
        });
        loop {
            if interrupt.load(Ordering::SeqCst) {
                // The worker is abandoned; it ends at its read timeout.
                return Err(Error::Interrupted);
            }
            match rx.recv_timeout(POLL_INTERVAL) {
                Ok(result) => return result,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(self.error("request thread died".into(), true))
                }
            }
        }
    }
}
