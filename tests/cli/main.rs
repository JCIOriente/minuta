//! Black-box tests: every test that runs the `minuta` binary. Some use real
//! `ffmpeg`/`ffprobe` on generated clips and the fake `whisper-cli` in `tests/fixtures/bin`
//! (`common::Env`), and skip when ffmpeg is not installed. The others put fake `ffmpeg`,
//! `ffprobe` and `whisper-cli` scripts on `PATH` (`fake_tools::FakeEnv`). `config_init` and
//! `templates_pull` need no tools and keep their own `Env`, which sets only `HOME`. `e2e` and
//! `e2e_llm` are opt-in.

#[cfg(unix)]
mod chapters;
mod common;
#[cfg(unix)]
mod config;
mod config_init;
#[cfg(unix)]
mod doctor;
#[cfg(unix)]
mod e2e;
#[cfg(unix)]
mod e2e_llm;
#[cfg(unix)]
mod endpoints;
#[cfg(unix)]
mod fake_tools;
#[cfg(unix)]
mod fields;
#[cfg(unix)]
mod force;
#[cfg(unix)]
mod interrupt;
#[cfg(unix)]
mod layout;
#[cfg(unix)]
mod llm_runs;
#[cfg(unix)]
mod models;
#[cfg(unix)]
mod pipeline;
#[cfg(unix)]
mod preflight;
#[cfg(unix)]
mod run;
#[cfg(unix)]
mod templates;
#[cfg(unix)]
mod templates_cmd;
mod templates_pull;
#[cfg(unix)]
mod vad;
mod version;
