use std::io::Write;
use std::net::TcpListener;
use std::thread;

use super::*;
use crate::test_support::fake_http::{FakeHttp, Route};

#[test]
fn endpoint_refused_connection_is_transient() {
    let err = client_at(CLOSED, None)
        .complete("SYS", "USER", &no_interrupt())
        .unwrap_err();
    let (message, transient) = llm_err(err);
    assert!(message.starts_with("request failed: "), "{message}");
    assert!(transient);
}

#[test]
fn endpoint_chat_refuses_an_invalid_port() {
    let url = "http://127.0.0.1:99999";
    let backend = OpenAiCompatible::new("box", "m", url, None, 1000, Duration::from_secs(5));
    let (message, transient) = llm_err(backend.complete("s", "u", &no_interrupt()).unwrap_err());
    assert_eq!(message, "request failed: invalid port number");
    assert!(!transient);
}

/// A listener that answers every connection with `reply`, one chunk every `pause`, and keeps the
/// connection open until the client closes it.
fn raw_server(reply: Vec<Vec<u8>>, pause: Duration) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            for (index, chunk) in reply.iter().enumerate() {
                if index > 0 {
                    thread::sleep(pause);
                }
                if stream.write_all(chunk).is_err() {
                    break;
                }
            }
            let _ = std::io::copy(&mut stream, &mut std::io::sink());
        }
    });
    url
}

#[test]
fn endpoint_tls_rejected_by_the_client_is_transient() {
    let url = raw_server(vec![b"HTTP/1.1 200 OK\r\n\r\n".to_vec()], Duration::ZERO);
    let url = url.replacen("http://", "https://", 1);
    let backend = OpenAiCompatible::new("box", "m", &url, None, 1000, Duration::from_secs(5));
    let (message, transient) = llm_err(backend.complete("s", "u", &no_interrupt()).unwrap_err());
    assert_eq!(
        message,
        format!(
            "request failed: {url}/chat/completions: io: received corrupt message of type \
             InvalidContentType"
        )
    );
    assert!(transient, "{message}");
}

/// The timeout bounds each read, not the body: a slow reply that keeps coming must finish.
#[test]
fn endpoint_read_timeout_restarts_with_each_read() {
    let body =
        json!({"choices": [{"message": {"content": "hi"}, "finish_reason": "stop"}]}).to_string();
    let mut reply = vec![format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes()];
    let pieces = body.len().div_ceil(15);
    reply.extend(body.as_bytes().chunks(pieces).map(<[u8]>::to_vec));
    let url = raw_server(reply, Duration::from_millis(200));
    let backend = OpenAiCompatible::new("box", "m", &url, None, 1000, Duration::from_secs(2));
    assert_eq!(backend.complete("s", "u", &no_interrupt()).unwrap(), "hi");
}

#[test]
fn endpoint_key_in_the_url_is_redacted_from_a_transport_error() {
    let key = "k3yk3yk3y";
    let url = format!("{}/{key}/v1", raw_server(vec![], Duration::ZERO));
    let url = url.replacen("http://", "https://", 1);
    let backend = OpenAiCompatible::new(
        "box",
        "m",
        &url,
        Some(key.into()),
        1000,
        Duration::from_millis(200),
    );
    let (message, _) = llm_err(backend.complete("s", "u", &no_interrupt()).unwrap_err());
    let shown = url.replace(key, "[redacted]");
    assert_eq!(
        message,
        format!("request failed: {shown}/chat/completions: timeout: per read")
    );
}

fn chat_reply(content: &str) -> String {
    format!(r#"{{"choices":[{{"message":{{"content":"{content}"}},"finish_reason":"stop"}}]}}"#)
}

/// A chat reply of exactly `size` bytes.
fn reply_of(size: usize) -> Vec<u8> {
    chat_reply(&"a".repeat(size - chat_reply("").len())).into_bytes()
}

const TEN_MIB: usize = 10 * 1024 * 1024;

#[test]
fn endpoint_reads_a_reply_of_ten_mib() {
    let body = reply_of(TEN_MIB);
    let server = FakeHttp::start(vec![Route::ok("/templates/chat/completions", body)]);
    let backend = OpenAiCompatible::new(
        "box",
        "m",
        &server.base(),
        None,
        1000,
        Duration::from_secs(5),
    );
    let reply = backend.complete("s", "u", &no_interrupt()).unwrap();
    assert_eq!(reply.len(), TEN_MIB - chat_reply("").len());
}

#[test]
fn endpoint_refuses_a_reply_over_ten_mib() {
    let body = reply_of(TEN_MIB + 1);
    let server = FakeHttp::start(vec![Route::ok("/templates/chat/completions", body)]);
    let backend = OpenAiCompatible::new(
        "box",
        "m",
        &server.base(),
        None,
        1000,
        Duration::from_secs(5),
    );
    let (message, transient) = llm_err(backend.complete("s", "u", &no_interrupt()).unwrap_err());
    assert_eq!(
        message,
        "unreadable response: the response body is larger than request limit: 10485761"
    );
    assert!(transient);
}

#[test]
fn endpoint_read_timeout_is_transient() {
    let fake = server(|_| Reply::Hang);
    let c = OpenAiCompatible::new(
        "box",
        "m",
        &format!("{}/v1", fake.url()),
        None,
        4096,
        Duration::from_millis(200),
    );
    let (message, transient) = llm_err(c.complete("SYS", "USER", &no_interrupt()).unwrap_err());
    assert_eq!(
        message,
        format!(
            "request failed: {}/v1/chat/completions: timeout: per read",
            fake.url()
        )
    );
    assert!(transient);
}
