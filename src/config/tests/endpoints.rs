//! `[endpoints.<name>]` tables: defaults, validation, host locality and entries.

use super::*;

fn with_endpoints(text: &str) -> Result<Config> {
    let dir = tempfile::tempdir().unwrap();
    let path = write_config(&dir, text);
    Config::resolve(Overrides::default(), &no_env, Some(&path))
}

#[test]
fn endpoint_config_defaults() {
    let cfg = with_endpoints("[endpoints.lmstudio]\nurl = \"http://127.0.0.1:1234/v1\"\n").unwrap();
    let endpoint = &cfg.endpoints["lmstudio"];
    assert_eq!(endpoint.url.value, "http://127.0.0.1:1234/v1");
    assert_eq!(endpoint.url.source, Source::File);
    assert_eq!(endpoint.context_tokens(), 16384);
    assert_eq!(endpoint.context_tokens.source, Source::Default);
    assert_eq!(endpoint.timeout(), Duration::from_secs(600));
    assert_eq!(endpoint.timeout_secs.source, Source::Default);
    assert_eq!(endpoint.api_key_env, None);
}

#[test]
fn endpoint_config_reads_every_key() {
    let cfg = with_endpoints(
        "[endpoints.cloud]\nurl = \"https://api.example.com/v1\"\ncontext_tokens = 32768\n\
         timeout_secs = \"3600\"\napi_key_env = \"CLOUD_KEY\"\n",
    )
    .unwrap();
    let endpoint = &cfg.endpoints["cloud"];
    assert_eq!(endpoint.context_tokens(), 32768);
    assert_eq!(endpoint.context_tokens.source, Source::File);
    assert_eq!(endpoint.timeout(), Duration::from_secs(3600));
    assert_eq!(endpoint.timeout_secs.source, Source::File);
    assert_key(
        endpoint.api_key_env.as_ref(),
        Some(("CLOUD_KEY", Source::File)),
        "api_key_env",
    );
}

#[test]
fn endpoint_config_none_by_default() {
    let cfg = Config::resolve(Overrides::default(), &no_env, None).unwrap();
    assert!(cfg.endpoints.is_empty());
}

#[test]
fn endpoint_config_lookup_of_unknown_name_shows_the_toml_to_add() {
    let cfg = Config::resolve(Overrides::default(), &no_env, None).unwrap();
    let Err(Error::Usage(message)) = cfg.endpoint("lmstudio") else {
        panic!("expected a usage error");
    };
    assert_eq!(
        message,
        "unknown endpoint \"lmstudio\"; add it to the config file (`minuta config path`):\n  \
         [endpoints.lmstudio]\n  \
         url = \"http://127.0.0.1:1234/v1\"  # your server's base URL"
    );
}

#[test]
fn endpoint_config_rejects_reserved_names() {
    for name in ["ollama", "claude", "codex"] {
        let text = format!("[endpoints.{name}]\nurl = \"http://127.0.0.1:1/v1\"\n");
        assert_eq!(
            config_err(with_endpoints(&text)),
            format!("endpoint name \"{name}\" is reserved for a built-in backend"),
        );
    }
}

#[test]
fn endpoint_config_rejects_bad_names() {
    for name in ["Bad", "1a", "a.b", "a b", "-a"] {
        let text = format!("[endpoints.\"{name}\"]\nurl = \"http://127.0.0.1:1/v1\"\n");
        assert_eq!(
            config_err(with_endpoints(&text)),
            format!(
                "endpoint name \"{name}\" must start with a lowercase letter and use only \
                 lowercase letters, digits, `-` and `_`"
            ),
        );
    }
}

#[test]
fn endpoint_config_requires_url() {
    assert_eq!(
        config_err(with_endpoints("[endpoints.a]\ncontext_tokens = 1\n")),
        "endpoints.a.url is required"
    );
    assert_eq!(
        config_err(with_endpoints("[endpoints.a]\nurl = \"\"\n")),
        "endpoints.a.url is required"
    );
}

#[test]
fn endpoint_config_rejects_a_value_that_is_not_a_url() {
    assert_eq!(
        config_err(with_endpoints("[endpoints.a]\nurl = \"nope\"\n")),
        "endpoints.a.url is not an http or https URL"
    );
}

#[test]
fn endpoint_config_rejects_other_schemes() {
    for url in [
        "ftp://example.com/v1?key=s3cret",
        "file:///tmp/x",
        "unix:/tmp/x",
        "localhost:1234/v1?key=s3cret",
    ] {
        let text = format!("[endpoints.a]\nurl = \"{url}\"\n");
        assert_eq!(
            config_err(with_endpoints(&text)),
            "endpoints.a.url is not an http or https URL",
        );
    }
}

#[test]
fn endpoint_config_rejects_credentials_without_echoing_them() {
    let err = config_err(with_endpoints(
        "[endpoints.a]\nurl = \"https://me:s3cret@example.com/v1\"\n",
    ));
    assert_eq!(
        err,
        "endpoints.a.url must not hold credentials; use endpoints.a.api_key_env"
    );
}

#[test]
fn endpoint_config_rejects_a_query_or_fragment_without_echoing_it() {
    for url in [
        "https://gw.example.com/v1?key=s3cret",
        "https://gw.example.com/v1#s3cret",
    ] {
        let text = format!("[endpoints.a]\nurl = \"{url}\"\n");
        assert_eq!(
            config_err(with_endpoints(&text)),
            "endpoints.a.url must not hold a query or fragment; use endpoints.a.api_key_env \
             for a key",
        );
    }
}

#[test]
fn endpoint_config_accepts_variable_names() {
    for name in ["OPENROUTER_API_KEY", "_key", "k3y"] {
        let text =
            format!("[endpoints.a]\nurl = \"http://127.0.0.1:1/v1\"\napi_key_env = \"{name}\"\n");
        with_endpoints(&text).unwrap();
    }
}

#[test]
fn endpoint_config_rejects_a_key_in_api_key_env_without_echoing_it() {
    for value in ["sk-or-s3cret", "1abc", "a b", "a$b"] {
        let text =
            format!("[endpoints.a]\nurl = \"http://127.0.0.1:1/v1\"\napi_key_env = \"{value}\"\n");
        assert_eq!(
            config_err(with_endpoints(&text)),
            "endpoints.a.api_key_env must be the name of an environment variable, not the key",
        );
    }
}

#[test]
fn endpoint_config_rejects_zero_context_tokens() {
    assert_eq!(
        config_err(with_endpoints(
            "[endpoints.a]\nurl = \"http://127.0.0.1:1/v1\"\ncontext_tokens = 0\n"
        )),
        "endpoints.a.context_tokens must be an integer of at least 1, got \"0\""
    );
}

#[test]
fn endpoint_config_rejects_zero_timeout_secs() {
    assert_eq!(
        config_err(with_endpoints(
            "[endpoints.a]\nurl = \"http://127.0.0.1:1/v1\"\ntimeout_secs = 0\n"
        )),
        "endpoints.a.timeout_secs must be an integer of at least 1, got \"0\""
    );
}

#[test]
fn endpoint_config_rejects_unknown_key() {
    let err = config_err(with_endpoints(
        "[endpoints.a]\nurl = \"http://127.0.0.1:1/v1\"\napi_key = \"x\"\n",
    ));
    assert!(err.contains("unknown field `api_key`"), "{err}");
}

#[test]
fn endpoint_host_is_local_for_loopback() {
    for url in [
        "http://localhost:1234/v1",
        "http://LOCALHOST/v1",
        "http://127.0.0.1:1234/v1",
        "http://127.8.9.1/v1",
        "http://[::1]:1234/v1",
    ] {
        let cfg = with_endpoints(&format!("[endpoints.a]\nurl = \"{url}\"\n")).unwrap();
        assert!(cfg.endpoints["a"].is_local(), "{url}");
    }
}

#[test]
fn endpoint_host_is_remote_otherwise() {
    for (url, host) in [
        ("https://openrouter.ai/api/v1", "openrouter.ai"),
        ("http://192.168.1.20:1234/v1", "192.168.1.20"),
        ("http://10.0.0.2/v1", "10.0.0.2"),
        ("http://localhost.example.com/v1", "localhost.example.com"),
        ("http://127.0.0.1.example.com/v1", "127.0.0.1.example.com"),
        ("http://[2001:db8::1]/v1", "[2001:db8::1]"),
    ] {
        let cfg = with_endpoints(&format!("[endpoints.a]\nurl = \"{url}\"\n")).unwrap();
        let endpoint = &cfg.endpoints["a"];
        assert!(!endpoint.is_local(), "{url}");
        assert_eq!(endpoint.host(), host, "{url}");
    }
}

#[test]
fn endpoint_config_entries_list_each_key_in_order() {
    let cfg = with_endpoints(
        "[endpoints.b]\nurl = \"http://127.0.0.1:1/v1\"\n\
         [endpoints.a]\nurl = \"http://127.0.0.1:2/v1\"\napi_key_env = \"A_KEY\"\n",
    )
    .unwrap();
    let entries: Vec<(String, Option<(String, Source)>)> = cfg
        .endpoint_entries()
        .into_iter()
        .map(|(key, setting)| (key, setting.map(|s| (s.value.clone(), s.source))))
        .collect();
    let set = |key: &str, value: &str, source| (key.to_string(), Some((value.to_string(), source)));
    let want = [
        set("endpoints.a.url", "http://127.0.0.1:2/v1", Source::File),
        set("endpoints.a.context_tokens", "16384", Source::Default),
        set("endpoints.a.timeout_secs", "600", Source::Default),
        set("endpoints.a.api_key_env", "A_KEY", Source::File),
        set("endpoints.b.url", "http://127.0.0.1:1/v1", Source::File),
        set("endpoints.b.context_tokens", "16384", Source::Default),
        set("endpoints.b.timeout_secs", "600", Source::Default),
        ("endpoints.b.api_key_env".to_string(), None),
    ];
    assert_eq!(entries, want);
}
