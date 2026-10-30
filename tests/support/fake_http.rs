//! Minimal fake HTTP file server for tests: HTTP/1.1 over `TcpListener`, `Connection: close`,
//! one thread per connection. Answers GET for the routes given to `start`, 404 for the rest.
#![allow(dead_code)] // Each test crate uses a different subset of the helpers.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

pub struct Route {
    path: String,
    status: u16,
    body: Vec<u8>,
    location: Option<String>,
}

impl Route {
    /// 200 with `body`.
    pub fn ok(path: &str, body: impl Into<Vec<u8>>) -> Route {
        Route::status(path, 200, body)
    }

    /// Any status with `body`.
    pub fn status(path: &str, status: u16, body: impl Into<Vec<u8>>) -> Route {
        Route {
            path: path.to_string(),
            status,
            body: body.into(),
            location: None,
        }
    }

    /// 302 to `to` (a path on this server).
    pub fn redirect(path: &str, to: &str) -> Route {
        Route {
            path: path.to_string(),
            status: 302,
            body: Vec::new(),
            location: Some(to.to_string()),
        }
    }
}

pub struct FakeHttp {
    addr: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
}

impl FakeHttp {
    pub fn start(routes: Vec<Route>) -> FakeHttp {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake http");
        let addr = listener.local_addr().expect("local addr").to_string();
        let routes = Arc::new(routes);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (routes_t, requests_t, stop_t) = (routes, requests.clone(), stop.clone());
        thread::spawn(move || {
            for stream in listener.incoming() {
                if stop_t.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let (routes, requests) = (routes_t.clone(), requests_t.clone());
                thread::spawn(move || {
                    if let Err(e) = handle(stream, &routes, &requests) {
                        eprintln!("fake http: connection error: {e}");
                    }
                });
            }
        });
        FakeHttp {
            addr,
            requests,
            stop,
        }
    }

    /// `http://127.0.0.1:PORT/templates`: the origin the tests point minuta at.
    pub fn base(&self) -> String {
        format!("http://{}/templates", self.addr)
    }

    /// Request paths received so far, in arrival order.
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests mutex").clone()
    }
}

impl Drop for FakeHttp {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(&self.addr); // wake the accept loop
    }
}

fn handle(
    stream: TcpStream,
    routes: &[Route],
    requests: &Mutex<Vec<String>>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let path = line
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_string();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        if header.trim_end().is_empty() {
            break;
        }
    }
    requests.lock().expect("requests mutex").push(path.clone());
    let mut stream = stream;
    let route = routes.iter().find(|r| r.path == path);
    let (status, body, location) = match route {
        Some(r) => (r.status, r.body.as_slice(), r.location.as_deref()),
        None => (404, b"not found".as_slice(), None),
    };
    write!(
        stream,
        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    )?;
    if let Some(to) = location {
        write!(stream, "Location: {to}\r\n")?;
    }
    stream.write_all(b"\r\n")?;
    stream.write_all(body)?;
    stream.flush()
}
