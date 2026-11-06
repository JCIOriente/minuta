//! OpenAI-compatible endpoints: runs, API keys, `doctor` and `config show`.

use std::fs;
use std::sync::Arc;

use assert_cmd::assert::OutputAssertExt;

use crate::common::fake_ollama::{FakeOllama, Reply};
use crate::common::*;
use crate::fake_tools::*;

const ENDPOINT_MODEL: &str = "google/gemma-4";

/// A fake OpenAI-compatible server offering [`ENDPOINT_MODEL`], and a config naming it `box`.
fn endpoint_server(env: &FakeEnv, extra: &str) -> FakeOllama {
    let fake = FakeOllama::start_with_show(
        &[],
        None,
        Arc::new(|request| {
            if request.path == "/v1/models" {
                let body = serde_json::json!({"data": [{"id": ENDPOINT_MODEL}]});
                return Reply::Status(200, body.to_string());
            }
            let body = serde_json::json!({"choices": [{
                "message": {"content": "endpoint reply"},
                "finish_reason": "stop",
            }]});
            Reply::Status(200, body.to_string())
        }),
    );
    let text = format!("[endpoints.box]\nurl = \"{}/v1\"\n{extra}", fake.url());
    fs::write(env.path("config.toml"), text).unwrap();
    fake
}

#[test]
fn run_with_endpoint_writes_output() {
    let env = FakeEnv::new();
    let fake = endpoint_server(&env, "");
    let id = format!("box:{ENDPOINT_MODEL}");
    let out = env.minuta_llm(
        &["run", "clase-4.mp4", "-t", "summary", "--llm", &id],
        &fake,
    );
    out.clone().assert().success();
    assert_eq!(
        fs::read_to_string(env.job_dir("clase-4").join("summary.md")).unwrap(),
        "endpoint reply\n"
    );
    let err = stderr(&out);
    assert!(!err.contains("sends its input"), "{err}");
    let chat: Vec<_> = fake
        .requests()
        .into_iter()
        .filter(|r| r.path == "/v1/chat/completions")
        .collect();
    assert_eq!(chat.len(), 1);
    assert_eq!(chat[0].body["model"], ENDPOINT_MODEL);
    assert_eq!(chat[0].authorization, None);
}

#[test]
fn run_with_endpoint_sends_the_key_from_its_variable() {
    let env = FakeEnv::new();
    let fake = endpoint_server(&env, "api_key_env = \"BOX_KEY\"\n");
    let id = format!("box:{ENDPOINT_MODEL}");
    let mut cmd = env.llm_command(
        &["run", "clase-4.mp4", "-t", "summary", "--llm", &id],
        &fake,
    );
    cmd.env("BOX_KEY", "s3cret-key");
    let out = output(&mut cmd);
    out.clone().assert().success();
    for request in fake.requests() {
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer s3cret-key"),
            "{}",
            request.path
        );
    }
    assert!(!stderr(&out).contains("s3cret-key"));
    assert!(!stdout(&out).contains("s3cret-key"));
}

#[test]
fn endpoint_key_variable_unset_is_reported_with_hint() {
    let env = FakeEnv::new();
    let fake = endpoint_server(&env, "api_key_env = \"BOX_KEY\"\n");
    let id = format!("box:{ENDPOINT_MODEL}");
    let out = env.minuta_llm(
        &["run", "clase-4.mp4", "-t", "summary", "--llm", &id],
        &fake,
    );
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(
        err.contains("box: set BOX_KEY to the API key (endpoints.box.api_key_env)"),
        "{err}"
    );
    assert!(fake.requests().is_empty());
}

#[test]
fn endpoint_malformed_key_is_refused_without_printing_it() {
    for key in ["s3cret\r", "s3 cret", "s3cr\u{e9}t"] {
        let env = FakeEnv::new();
        let fake = endpoint_server(&env, "api_key_env = \"BOX_KEY\"\n");
        let id = format!("box:{ENDPOINT_MODEL}");
        let mut cmd = env.llm_command(
            &["run", "clase-4.mp4", "-t", "summary", "--llm", &id],
            &fake,
        );
        cmd.env("BOX_KEY", key);
        let out = output(&mut cmd);
        assert!(!out.status.success(), "{key:?}");
        let err = stderr(&out);
        assert!(
            err.contains("box: BOX_KEY is set, but holds whitespace or non-ASCII characters"),
            "{key:?}: {err}"
        );
        assert!(!err.contains("s3"), "{key:?}: {err}");
        assert!(fake.requests().is_empty(), "{key:?}");
    }
}

#[test]
fn endpoint_key_empty_is_reported_with_hint() {
    let env = FakeEnv::new();
    let fake = endpoint_server(&env, "api_key_env = \"BOX_KEY\"\n");
    let id = format!("box:{ENDPOINT_MODEL}");
    let mut cmd = env.llm_command(
        &["run", "clase-4.mp4", "-t", "summary", "--llm", &id],
        &fake,
    );
    cmd.env("BOX_KEY", "");
    let out = output(&mut cmd);
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(
        err.contains("box: set BOX_KEY to the API key (endpoints.box.api_key_env)"),
        "{err}"
    );
    assert!(fake.requests().is_empty());
}

#[test]
fn endpoint_key_that_is_not_utf8_is_reported_with_hint() {
    use std::os::unix::ffi::OsStrExt;
    let env = FakeEnv::new();
    let fake = endpoint_server(&env, "api_key_env = \"BOX_KEY\"\n");
    let id = format!("box:{ENDPOINT_MODEL}");
    let mut cmd = env.llm_command(
        &["run", "clase-4.mp4", "-t", "summary", "--llm", &id],
        &fake,
    );
    cmd.env("BOX_KEY", std::ffi::OsStr::from_bytes(b"\xff"));
    let out = output(&mut cmd);
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(
        err.contains("box: BOX_KEY is set, but holds whitespace or non-ASCII characters"),
        "{err}"
    );
    assert!(fake.requests().is_empty());
}

#[test]
fn unknown_endpoint_is_reported_with_toml() {
    let env = FakeEnv::new();
    let fake = ollama("unused");
    let out = env.minuta_llm(
        &["run", "clase-4.mp4", "-t", "summary", "--llm", "lmstudio:m"],
        &fake,
    );
    assert!(!out.status.success());
    assert_eq!(
        stderr(&out),
        "minuta: usage error: unknown endpoint \"lmstudio\"; add it to the config file (`minuta config path`):\n  \
         [endpoints.lmstudio]\n  \
         url = \"http://127.0.0.1:1234/v1\"  # your server's base URL\n"
    );
    assert!(!env.path("minuta-out").exists());
}

#[test]
fn unknown_endpoint_is_reported_without_templates() {
    let env = FakeEnv::new();
    let fake = ollama("unused");
    let out = env.minuta_llm(&["run", "clase-4.mp4", "--llm", "lmstudio:m"], &fake);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert_eq!(
        stderr(&out),
        "minuta: usage error: unknown endpoint \"lmstudio\"; add it to the config file (`minuta config path`):\n  \
         [endpoints.lmstudio]\n  \
         url = \"http://127.0.0.1:1234/v1\"  # your server's base URL\n"
    );
    assert!(!env.path("minuta-out").exists());
}

#[test]
fn unlisted_endpoint_model_fails_before_any_call() {
    let env = FakeEnv::new();
    let fake = endpoint_server(&env, "");
    let out = env.minuta_llm(
        &["run", "clase-4.mp4", "-t", "summary", "--llm", "box:nope"],
        &fake,
    );
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(
        err.contains(&format!(
            "box:nope: not offered by the server; it offers: {ENDPOINT_MODEL}"
        )),
        "{err}"
    );
    assert!(fake.requests().iter().all(|r| r.path == "/v1/models"));
}

fn doctor_llm_line(env: &FakeEnv, fake: &FakeOllama, id: &str) -> String {
    let mut cmd = env.llm_command(&["doctor"], fake);
    cmd.env("MINUTA_LLM", id);
    let out = output(&mut cmd);
    stdout(&out)
        .lines()
        .find(|line| line.starts_with("llm "))
        .expect("doctor prints the llm line")
        .to_string()
}

#[test]
fn doctor_reports_endpoint_ok() {
    let env = FakeEnv::new();
    let fake = endpoint_server(&env, "");
    assert_eq!(
        doctor_llm_line(&env, &fake, &format!("box:{ENDPOINT_MODEL}")),
        format!("llm          box:{ENDPOINT_MODEL} (ok)")
    );
}

#[test]
fn doctor_reports_endpoint_model_missing() {
    let env = FakeEnv::new();
    let fake = endpoint_server(&env, "");
    assert_eq!(
        doctor_llm_line(&env, &fake, "box:nope"),
        format!(
            "llm          box:nope (model missing: not offered by the server; it offers: \
             {ENDPOINT_MODEL})"
        )
    );
}

#[test]
fn doctor_reports_endpoint_not_reachable() {
    let env = FakeEnv::new();
    let fake = ollama("unused");
    fs::write(
        env.path("config.toml"),
        "[endpoints.box]\nurl = \"http://127.0.0.1:9/v1\"\n",
    )
    .unwrap();
    let line = doctor_llm_line(&env, &fake, "box:m");
    assert!(
        line.starts_with(
            "llm          box:m (not reachable: start the server or fix endpoints.box.url; "
        ),
        "{line}"
    );
}

#[test]
fn doctor_reports_unknown_endpoint_as_invalid() {
    let env = FakeEnv::new();
    let fake = ollama("unused");
    let line = doctor_llm_line(&env, &fake, "box:m");
    assert_eq!(
        line,
        "llm          box:m (invalid: unknown endpoint \"box\"; add it to the config file (`minuta config path`):"
    );
}

#[test]
fn config_show_lists_endpoints() {
    let env = FakeEnv::new();
    fs::write(
        env.path("config.toml"),
        "[endpoints.box]\nurl = \"http://127.0.0.1:1234/v1\"\ntimeout_secs = 3600\n\
         api_key_env = \"BOX_KEY\"\n",
    )
    .unwrap();
    let mut cmd = env.command(&["config", "show"]);
    cmd.env("BOX_KEY", "s3cret-key");
    let out = output(&mut cmd);
    out.clone().assert().success();
    let shown = stdout(&out);
    assert!(
        shown.ends_with(
            "endpoints.box.url = http://127.0.0.1:1234/v1 (file)\n\
             endpoints.box.context_tokens = 16384 (default)\n\
             endpoints.box.timeout_secs = 3600 (file)\n\
             endpoints.box.api_key_env = BOX_KEY (file)\n"
        ),
        "{shown}"
    );
    assert!(!shown.contains("s3cret-key"), "{shown}");
}

#[test]
fn config_show_marks_an_unset_endpoint_key() {
    let env = FakeEnv::new();
    fs::write(
        env.path("config.toml"),
        "[endpoints.box]\nurl = \"http://127.0.0.1:1234/v1\"\n",
    )
    .unwrap();
    let out = env.minuta(&["config", "show"]);
    out.clone().assert().success();
    let shown = stdout(&out);
    assert!(
        shown.ends_with("endpoints.box.api_key_env = (unset)\n"),
        "{shown}"
    );
}

#[test]
fn llm_help_names_the_endpoint_form() {
    let env = FakeEnv::new();
    let out = env.minuta(&["run", "--help"]);
    let help = stdout(&out);
    assert!(help.contains("<endpoint>:<model>"), "{help}");
    assert!(help.contains("[endpoints.<endpoint>]"), "{help}");
}
