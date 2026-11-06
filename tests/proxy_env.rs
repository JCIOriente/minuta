//! No HTTP client reads the proxy variables: API keys and transcripts must go to the host the
//! config names. This binary of its own holds the tests that set them for the whole process.

use std::sync::{Arc, Once};

use minuta::llm::{endpoint, ollama};
use minuta::models::{self, ModelKind, ModelSpec, Pulled};
use minuta::template::pull::{fetch_catalog, Origin};
use serde_json::json;
use sha2::{Digest, Sha256};

#[path = "../src/test_support/fake_http.rs"]
mod fake_http;
use fake_http::{FakeHttp, Route};

#[path = "../src/test_support/fake_ollama.rs"]
mod fake_ollama;
use fake_ollama::{FakeOllama, Reply};

/// Nothing listens here, so a client that uses the proxy fails instead of reaching the server.
const DEAD_PROXY: &str = "http://127.0.0.1:1";

fn point_proxies_at_nothing() {
    static SET: Once = Once::new();
    SET.call_once(|| {
        for var in ["ALL_PROXY", "HTTP_PROXY", "HTTPS_PROXY"] {
            std::env::set_var(var, DEAD_PROXY);
            std::env::set_var(var.to_lowercase(), DEAD_PROXY);
        }
        std::env::remove_var("NO_PROXY");
        std::env::remove_var("no_proxy");
    });
}

#[test]
fn endpoint_check_ignores_proxy_variables() {
    point_proxies_at_nothing();
    let server = FakeHttp::start(vec![Route::ok(
        "/templates/models",
        json!({"data": [{"id": "m"}]}).to_string(),
    )]);
    assert!(endpoint::check("box", &server.base(), None, "m").is_ok());
    assert_eq!(server.requests(), vec!["/templates/models"]);
}

#[test]
fn ollama_check_ignores_proxy_variables() {
    point_proxies_at_nothing();
    let show = json!({"model_info": {}, "capabilities": []});
    let server = FakeOllama::start(&["m"], show, Arc::new(|_| Reply::chat("hello")));
    assert!(ollama::check(&server.url(), "m").is_ok());
}

#[test]
fn model_download_ignores_proxy_variables() {
    point_proxies_at_nothing();
    let body = b"weights";
    let server = FakeHttp::start(vec![Route::ok("/templates/m.bin", body.as_slice())]);
    let dir = tempfile::tempdir().unwrap();
    let sha: String = Sha256::digest(body)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let spec = ModelSpec {
        name: "m",
        file: "m.bin",
        kind: ModelKind::Whisper,
        base_url: &server.base(),
        sha256: &sha,
        size: body.len() as u64,
    };
    assert_eq!(models::pull(&spec, dir.path()).unwrap(), Pulled::Downloaded);
}

#[test]
fn template_fetch_ignores_proxy_variables() {
    point_proxies_at_nothing();
    let server = FakeHttp::start(vec![Route::ok("/templates/index.toml", "nonsense = [")]);
    let err = fetch_catalog(&Origin::new(&server.base()).unwrap()).unwrap_err();
    assert_eq!(server.requests(), vec!["/templates/index.toml"]);
    assert!(
        !err.to_string().contains("download"),
        "never reached the server: {err}"
    );
}
