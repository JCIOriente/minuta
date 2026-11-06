//! Helpers shared by the unit tests. The two fakes use only `std` and `serde_json`, never
//! `crate::`, so the test crates in `tests/` include the same files with `#[path]`.

pub(crate) mod fake_http;
pub(crate) mod fake_ollama;
