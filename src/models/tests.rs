//! Tests for model download, lookup and listing, split by behavior.

use crate::models::{self, ModelKind, ModelSpec, Pulled};
use crate::test_support::fake_http::{FakeHttp, Route};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

mod catalog;
mod download;
mod integrity;
mod list;
mod progress;
mod resolve;

const BODY: &[u8] = b"pretend these are model weights";

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        })
}

type Requests = Arc<Mutex<Vec<String>>>;

/// Serves `body` at `/ggml-tiny.bin` (404 for any other path) on a local port; returns the
/// base URL and the request lines received.
fn serve(body: &'static [u8]) -> (String, Requests) {
    serve_at("/ggml-tiny.bin", body)
}

/// Serves `body` at `path` (404 for any other path).
fn serve_at(path: &'static str, body: &'static [u8]) -> (String, Requests) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let requests: Requests = Arc::default();
    let log = requests.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut seen = Vec::new();
            let mut buf = [0u8; 512];
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                seen.extend_from_slice(&buf[..n]);
            }
            let line = String::from_utf8_lossy(&seen)
                .lines()
                .next()
                .unwrap_or_default()
                .to_string();
            let found = line.starts_with(&format!("GET {path} "));
            log.lock().unwrap().push(line);
            let (status, payload) = if found {
                ("200 OK", body)
            } else {
                ("404 Not Found", &b"missing"[..])
            };
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(payload).unwrap();
        }
    });
    (base, requests)
}

fn files_in(dir: &Path) -> Vec<String> {
    match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => Vec::new(),
    }
}
