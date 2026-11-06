//! Tests for the Ollama backend, one file per topic. Helpers shared by several topics live here.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::error::Error;
use crate::llm::ollama::OllamaInfo;
use serde_json::{json, Value};

use crate::test_support::fake_ollama::{Reply, Responder};

mod chat;
mod check;
mod complete_json;
mod errors;

fn show(context: Option<u64>, thinking: bool) -> Value {
    let mut info = json!({"general.architecture": "gemma4"});
    if let Some(n) = context {
        info["gemma4.context_length"] = json!(n);
    }
    let caps = if thinking {
        json!(["completion", "thinking"])
    } else {
        json!(["completion"])
    };
    json!({"model_info": info, "capabilities": caps})
}

fn ok_chat() -> Responder {
    Arc::new(|_| Reply::chat("hello"))
}

fn info(context_length: Option<usize>, thinking: bool) -> OllamaInfo {
    OllamaInfo {
        context_length,
        thinking,
    }
}

fn no_interrupt() -> AtomicBool {
    AtomicBool::new(false)
}

fn llm_err(e: Error) -> (String, bool) {
    match e {
        Error::Llm {
            message, transient, ..
        } => (message, transient),
        other => panic!("expected Error::Llm, got {other}"),
    }
}

/// ureq 3 swaps a port it cannot parse for the scheme's default, so a request would go out.
const BAD_PORT: &str = "http://127.0.0.1:99999";
