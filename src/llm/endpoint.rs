//! OpenAI-compatible HTTP backend, for any server declared under `[endpoints.<name>]`.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::{json, Value};

use super::http::{self, base, describe, excerpt, read_json};
use super::Backend;
use crate::config::Endpoint;
use crate::error::{Error, Missing, Result};

const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
/// Model ids a hint lists, so a cloud catalog does not fill the terminal.
const MAX_LISTED_MODELS: usize = 10;

/// Why an endpoint cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    Unreachable(Missing),
    ModelMissing(Missing),
}

impl Problem {
    pub fn into_missing(self) -> Missing {
        match self {
            Problem::Unreachable(m) | Problem::ModelMissing(m) => m,
        }
    }
}

/// Reads the endpoint's key and checks the server offers `model`.
pub fn connect(
    name: &str,
    endpoint: &Endpoint,
    model: &str,
) -> std::result::Result<OpenAiCompatible, Problem> {
    let var = endpoint.api_key_env.as_ref().map(|s| s.value.as_str());
    let key = api_key(name, var).map_err(Problem::Unreachable)?;
    let url = &endpoint.url.value;
    check(name, url, key.as_deref(), model)?;
    Ok(OpenAiCompatible::new(
        name,
        model,
        url,
        key,
        endpoint.context_tokens(),
        endpoint.timeout(),
    ))
}

/// The key in `var`. Only visible ASCII passes: a header error would print the value.
fn api_key(name: &str, var: Option<&str>) -> std::result::Result<Option<String>, Missing> {
    let Some(var) = var else {
        return Ok(None);
    };
    std::env::var_os(var)
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.is_empty() && value.chars().all(|c| c.is_ascii_graphic()))
        .map(Some)
        .ok_or_else(|| Missing {
            name: name.to_string(),
            hint: format!("set {var} to the API key (endpoints.{name}.api_key_env)"),
        })
}

/// Verifies the server answers and lists `model`. A server may otherwise answer with whichever
/// model it has loaded, whatever the request asked for.
pub fn check(
    name: &str,
    url: &str,
    key: Option<&str>,
    model: &str,
) -> std::result::Result<(), Problem> {
    let base = base(url);
    let down = |why: String| {
        Problem::Unreachable(Missing {
            name: name.to_string(),
            hint: format!("start the server or fix endpoints.{name}.url; {why} at {base}"),
        })
    };
    let mut request = agent(CHECK_TIMEOUT).get(&format!("{base}/models"));
    if let Some(key) = key {
        request = request.set("Authorization", &format!("Bearer {key}"));
    }
    let response = match request.call() {
        Ok(r) if r.status() >= 300 => {
            return Err(down(format!(
                "HTTP status {} (redirects are not followed)",
                r.status()
            )))
        }
        Ok(r) => r,
        Err(ureq::Error::Status(code @ (401 | 403), _)) => {
            return Err(Problem::Unreachable(Missing {
                name: name.to_string(),
                hint: format!(
                    "HTTP status {code} at {base}; check the key in endpoints.{name}.api_key_env"
                ),
            }))
        }
        Err(e) => return Err(down(describe(&e))),
    };
    let listed = read_json(response)
        .map_err(|e| down(format!("unreadable /models response ({})", e.message())))?;
    let Some(models) = listed["data"].as_array() else {
        return Err(down(format!(
            "unexpected /models response ({})",
            excerpt(&listed.to_string())
        )));
    };
    let ids: Vec<&str> = models.iter().filter_map(|m| m["id"].as_str()).collect();
    if ids.contains(&model) {
        return Ok(());
    }
    Err(Problem::ModelMissing(Missing {
        name: format!("{name}:{model}"),
        hint: not_offered(&ids),
    }))
}

fn not_offered(ids: &[&str]) -> String {
    if ids.is_empty() {
        return "the server offers no models".to_string();
    }
    let shown: Vec<String> = ids
        .iter()
        .take(MAX_LISTED_MODELS)
        .map(|id| excerpt(id))
        .collect();
    let more = ids.len().saturating_sub(MAX_LISTED_MODELS);
    let tail = if more > 0 {
        format!(", and {more} more")
    } else {
        String::new()
    };
    format!(
        "not offered by the server; it offers: {}{tail}",
        shown.join(", ")
    )
}

/// Redirects stay off: following one could send a key or a transcript to another host.
fn agent(read_timeout: Duration) -> ureq::Agent {
    http::agent(read_timeout).redirects(0).build()
}

/// A failed request, with the HTTP status when the server answered one.
struct Failure {
    status: Option<u16>,
    error: Error,
}

#[derive(Clone)]
pub struct OpenAiCompatible {
    id: String,
    name: String,
    model: String,
    url: String,
    key: Option<String>,
    context_tokens: usize,
    timeout: Duration,
}

impl OpenAiCompatible {
    pub fn new(
        name: &str,
        model: &str,
        url: &str,
        key: Option<String>,
        context_tokens: usize,
        timeout: Duration,
    ) -> OpenAiCompatible {
        OpenAiCompatible {
            id: format!("{name}:{model}"),
            name: name.to_string(),
            model: model.to_string(),
            url: base(url).to_string(),
            key,
            context_tokens,
            timeout,
        }
    }

    fn error(&self, message: String, transient: bool) -> Error {
        Error::Llm {
            backend: self.id.clone(),
            message,
            transient,
        }
    }

    fn body(&self, system: &str, user: &str) -> Value {
        json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "stream": false,
        })
    }

    /// One blocking round trip; runs on a worker thread.
    fn post(&self, body: &Value) -> std::result::Result<String, Failure> {
        let failed = |error| Failure {
            status: None,
            error,
        };
        let mut request = agent(self.timeout)
            .post(&format!("{}/chat/completions", self.url))
            .set("Content-Type", "application/json");
        if let Some(key) = &self.key {
            request = request.set("Authorization", &format!("Bearer {key}"));
        }
        let response = match request.send_string(&body.to_string()) {
            Ok(r) if r.status() >= 300 => {
                let message = format!(
                    "HTTP {}; redirects are not followed, so set endpoints.{}.url to the final address",
                    r.status(),
                    self.name
                );
                return Err(failed(self.error(message, false)));
            }
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                return Err(Failure {
                    status: Some(code),
                    error: self.status_error(code, r),
                })
            }
            Err(ureq::Error::Transport(t)) => {
                let transient = http::transport_is_transient(&t);
                return Err(failed(
                    self.error(format!("request failed: {t}"), transient),
                ));
            }
        };
        let parsed = read_json(response).map_err(|e| {
            failed(self.error(
                format!("unreadable response: {}", e.message()),
                e.is_transient(),
            ))
        })?;
        let choice = &parsed["choices"][0];
        if choice["finish_reason"] == "length" {
            let message = format!(
                "response cut off at the context limit; give the model a larger context on the \
                 server, or lower endpoints.{}.context_tokens",
                self.name
            );
            return Err(failed(self.error(message, false)));
        }
        match choice["message"]["content"].as_str() {
            Some(content) if !content.trim().is_empty() => Ok(content.to_string()),
            _ => Err(failed(
                self.error("response has no message content".into(), true),
            )),
        }
    }

    fn status_error(&self, code: u16, response: ureq::Response) -> Error {
        let name = &self.name;
        let detail = match response.into_string() {
            Ok(text) => {
                let shown = error_message(&text).unwrap_or_else(|| excerpt(&text));
                if shown.is_empty() {
                    String::new()
                } else {
                    format!(": {shown}")
                }
            }
            Err(_) => String::new(),
        };
        match code {
            401 | 403 => self.error(
                format!("HTTP {code}{detail}; check the key in endpoints.{name}.api_key_env"),
                false,
            ),
            404 => self.error(
                format!("HTTP 404{detail}; check endpoints.{name}.url, which usually ends in /v1"),
                false,
            ),
            429 | 500..=599 => self.error(format!("HTTP {code}{detail}"), true),
            _ => self.error(format!("HTTP {code}{detail}"), false),
        }
    }
}

/// The message in an error body: `{"error": {"message": ...}}` (OpenAI, Ollama) or
/// `{"error": "..."}` (LM Studio).
fn error_message(text: &str) -> Option<String> {
    let parsed = serde_json::from_str::<Value>(text).ok()?;
    let error = &parsed["error"];
    error["message"].as_str().or(error.as_str()).map(excerpt)
}

impl Backend for OpenAiCompatible {
    fn id(&self) -> &str {
        &self.id
    }

    fn context_tokens(&self) -> usize {
        self.context_tokens
    }

    fn complete(&self, system: &str, user: &str, interrupt: &AtomicBool) -> Result<String> {
        let worker = self.clone();
        let body = self.body(system, user);
        http::on_worker(
            move || worker.post(&body).map_err(|failure| failure.error),
            interrupt,
            || self.error("request thread died".into(), true),
        )
    }

    fn complete_json(
        &self,
        system: &str,
        user: &str,
        schema: &Value,
        interrupt: &AtomicBool,
    ) -> Result<String> {
        let worker = self.clone();
        let plain = self.body(system, user);
        let mut structured = plain.clone();
        structured["response_format"] = json!({
            "type": "json_schema",
            "json_schema": {"name": "minuta", "strict": true, "schema": schema},
        });
        http::on_worker(
            move || match worker.post(&structured) {
                // A server without structured output refuses the field; templates work without it.
                Err(Failure {
                    status: Some(400 | 422),
                    ..
                }) => worker.post(&plain).map_err(|failure| failure.error),
                other => other.map_err(|failure| failure.error),
            },
            interrupt,
            || self.error("request thread died".into(), true),
        )
    }
}
