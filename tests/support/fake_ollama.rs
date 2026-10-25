//! Minimal fake Ollama server for tests: HTTP/1.1 over `TcpListener`, `Connection: close`,
//! one thread per connection. `/api/tags` and `/api/show` are served from the fixtures given
//! to `start`; every other request goes to the responder.
#![allow(dead_code)] // Each test crate uses a different subset of the helpers.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

/// A request as received. `chat_index` is the 0-based count of earlier `/api/chat` requests,
/// `None` for any other path.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub body: Value,
    pub chat_index: Option<usize>,
}

pub enum Reply {
    /// Any status with a raw body.
    Status(u16, String),
    /// Never answer; the connection is closed when the server is dropped.
    Hang,
}

impl Reply {
    /// 200 with a well-formed non-streaming chat response.
    pub fn chat(content: &str) -> Reply {
        Reply::chat_done(content, "stop")
    }

    pub fn chat_done(content: &str, done_reason: &str) -> Reply {
        let body = json!({
            "model": "fake",
            "message": {"role": "assistant", "content": content},
            "done": true,
            "done_reason": done_reason,
        });
        Reply::Status(200, body.to_string())
    }

    /// An Ollama-style error body: `{"error": message}`.
    pub fn error(status: u16, message: &str) -> Reply {
        Reply::Status(status, json!({ "error": message }).to_string())
    }
}

pub type Responder = Arc<dyn Fn(&Request) -> Reply + Send + Sync>;

pub struct FakeOllama {
    addr: String,
    requests: Arc<Mutex<Vec<Request>>>,
    chats: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
}

impl FakeOllama {
    /// `models` are the names `/api/tags` lists; `show_info` is the full `/api/show` body
    /// (for a listed model; unlisted ones get 404).
    pub fn start(models: &[&str], show_info: Value, responder: Responder) -> FakeOllama {
        FakeOllama::start_with_show(models, Some(show_info), responder)
    }

    /// Like `start`, but `/api/tags` answers 200 with `tags` verbatim.
    pub fn start_with_tags(tags: Value, responder: Responder) -> FakeOllama {
        FakeOllama::spawn(&[], None, Some(tags), responder)
    }

    /// Like `start`, but `show_info: None` makes `/api/show` fail with a 500.
    pub fn start_with_show(
        models: &[&str],
        show_info: Option<Value>,
        responder: Responder,
    ) -> FakeOllama {
        FakeOllama::spawn(models, show_info, None, responder)
    }

    fn spawn(
        models: &[&str],
        show_info: Option<Value>,
        tags: Option<Value>,
        responder: Responder,
    ) -> FakeOllama {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake ollama");
        let addr = listener.local_addr().expect("local addr").to_string();
        let names: Arc<Vec<String>> = Arc::new(models.iter().map(|m| m.to_string()).collect());
        let show_info = Arc::new(show_info);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let chats = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Shared {
            names,
            show_info,
            tags: Arc::new(tags),
            responder,
            requests: requests.clone(),
            chats: chats.clone(),
            stop: stop.clone(),
        };
        let stop_accept = stop.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                if stop_accept.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let shared = shared.clone();
                thread::spawn(move || {
                    if let Err(e) = handle(stream, &shared) {
                        eprintln!("fake ollama: connection error: {e}");
                    }
                });
            }
        });
        FakeOllama {
            addr,
            requests,
            chats,
            stop,
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Every request received so far, in arrival order.
    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().expect("requests mutex").clone()
    }

    /// Number of `/api/chat` requests received so far.
    pub fn chat_count(&self) -> usize {
        self.chats.load(Ordering::SeqCst)
    }
}

impl Drop for FakeOllama {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(&self.addr); // wake the accept loop
    }
}

#[derive(Clone)]
struct Shared {
    names: Arc<Vec<String>>,
    show_info: Arc<Option<Value>>,
    tags: Arc<Option<Value>>,
    responder: Responder,
    requests: Arc<Mutex<Vec<Request>>>,
    chats: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
}

fn handle(stream: TcpStream, shared: &Shared) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut length = 0usize;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut raw = vec![0u8; length];
    reader.read_exact(&mut raw)?;
    let body: Value = serde_json::from_slice(&raw)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&raw).into_owned()));

    let chat_index = (path == "/api/chat").then(|| shared.chats.fetch_add(1, Ordering::SeqCst));
    let request = Request {
        method,
        path,
        body,
        chat_index,
    };
    shared
        .requests
        .lock()
        .expect("requests mutex")
        .push(request.clone());

    let reply = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/api/tags") if shared.tags.is_some() => Reply::Status(
            200,
            shared.tags.as_ref().clone().unwrap_or_default().to_string(),
        ),
        ("GET", "/api/tags") => {
            let models: Vec<Value> = shared
                .names
                .iter()
                .map(|n| json!({"name": n, "model": n}))
                .collect();
            Reply::Status(200, json!({ "models": models }).to_string())
        }
        ("POST", "/api/show") => {
            let asked = request.body["model"].as_str().unwrap_or_default();
            let latest = format!("{asked}:latest"); // like Ollama, a bare name means :latest
            let listed = shared.names.iter().any(|n| n == asked || *n == latest);
            if let (true, Some(info)) = (listed, shared.show_info.as_ref()) {
                Reply::Status(200, info.to_string())
            } else if listed {
                Reply::error(500, "show failed")
            } else {
                Reply::error(404, &format!("model '{asked}' not found"))
            }
        }
        _ => (shared.responder)(&request),
    };
    let mut stream = stream;
    match reply {
        Reply::Status(status, body) => {
            write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )?;
            stream.flush()
        }
        Reply::Hang => {
            while !shared.stop.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(20));
            }
            Ok(())
        }
    }
}
