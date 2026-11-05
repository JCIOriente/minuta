//! OpenAI-compatible endpoints: the resolved `Endpoint` and the validation of its table.

use std::net::IpAddr;
use std::time::Duration;

use super::file::{EndpointTable, Scalar};
use super::{
    int_setting, parse_checked, pick, pick_opt, Setting, DEFAULT_ENDPOINT_CONTEXT_TOKENS,
    DEFAULT_ENDPOINT_TIMEOUT_SECS,
};
use crate::error::{Error, Result};
use crate::llm::{is_endpoint_name, RESERVED_ENDPOINT_NAMES};

/// An OpenAI-compatible server. Valid by construction in `Config::resolve`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub url: Setting,
    pub context_tokens: Setting,
    pub timeout_secs: Setting,
    /// The name of the variable holding the API key, never the key.
    pub api_key_env: Option<Setting>,
    host: String,
}

impl Endpoint {
    /// Prompt + output budget per call.
    pub fn context_tokens(&self) -> usize {
        parse_checked(&self.context_tokens)
    }

    /// Read timeout for one request.
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(parse_checked(&self.timeout_secs))
    }

    /// The URL's host, as the off-machine notice shows it.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// True when requests stay on this machine. A LAN address does not count.
    pub fn is_local(&self) -> bool {
        let unbracketed = self.host.trim_start_matches('[').trim_end_matches(']');
        self.host == "localhost"
            || unbracketed
                .parse::<IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    }
}

/// Validates one `[endpoints.<name>]` table.
pub(super) fn endpoint(name: &str, table: EndpointTable) -> Result<Endpoint> {
    if RESERVED_ENDPOINT_NAMES.contains(&name) {
        return Err(Error::Config(format!(
            "endpoint name {name:?} is reserved for a built-in backend"
        )));
    }
    if !is_endpoint_name(name) {
        return Err(Error::Config(format!(
            "endpoint name {name:?} must start with a lowercase letter and use only lowercase \
             letters, digits, `-` and `_`"
        )));
    }
    let url = pick_opt(None, None, table.url)
        .ok_or_else(|| Error::Config(format!("endpoints.{name}.url is required")))?;
    let host = endpoint_host(name, &url.value)?;
    let api_key_env = pick_opt(None, None, table.api_key_env);
    if api_key_env.as_ref().is_some_and(|s| !is_env_name(&s.value)) {
        return Err(Error::Config(format!(
            "endpoints.{name}.api_key_env must be the name of an environment variable, not the key"
        )));
    }
    let context_tokens = pick(
        None,
        None,
        table.context_tokens.map(Scalar::into_text),
        DEFAULT_ENDPOINT_CONTEXT_TOKENS,
    );
    let timeout_secs = pick(
        None,
        None,
        table.timeout_secs.map(Scalar::into_text),
        DEFAULT_ENDPOINT_TIMEOUT_SECS,
    );
    let key = format!("endpoints.{name}.context_tokens");
    int_setting::<usize>(&context_tokens, &key, 1)?;
    let key = format!("endpoints.{name}.timeout_secs");
    int_setting::<u64>(&timeout_secs, &key, 1)?;
    Ok(Endpoint {
        url,
        context_tokens,
        timeout_secs,
        api_key_env,
        host,
    })
}

/// Keeps a key pasted into `api_key_env` out of `config show` and `doctor`.
fn is_env_name(name: &str) -> bool {
    !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The host of an endpoint URL. Credentials, and a query or fragment that could hold them, are
/// refused without the URL reaching a message.
fn endpoint_host(name: &str, url: &str) -> Result<String> {
    if url.contains('@') {
        return Err(Error::Config(format!(
            "endpoints.{name}.url must not hold credentials; use endpoints.{name}.api_key_env"
        )));
    }
    let not_a_url = || Error::Config(format!("endpoints.{name}.url is not an http or https URL"));
    let parsed = url::Url::parse(url).map_err(|_| not_a_url())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(not_a_url());
    }
    // Paths are appended to the URL as text, so these would also swallow them.
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(Error::Config(format!(
            "endpoints.{name}.url must not hold a query or fragment; use \
             endpoints.{name}.api_key_env for a key"
        )));
    }
    parsed.host_str().map(str::to_string).ok_or_else(not_a_url)
}
