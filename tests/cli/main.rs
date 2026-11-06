//! Black-box tests: every test that runs the `minuta` binary. Most use real
//! `ffmpeg`/`ffprobe` on generated clips and the fake `whisper-cli` in `tests/fixtures/bin`,
//! and skip when ffmpeg is not installed. `e2e` and `e2e_llm` are opt-in.

#[cfg(unix)]
mod chapters;
mod common;
#[cfg(unix)]
mod e2e;
#[cfg(unix)]
mod e2e_llm;
#[cfg(unix)]
mod fields;
#[cfg(unix)]
mod layout;
#[cfg(unix)]
mod pipeline;
#[cfg(unix)]
mod templates;
mod version;
