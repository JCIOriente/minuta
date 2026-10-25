#![cfg(unix)]

//! The `minuta` binary end to end: real `ffmpeg`/`ffprobe` on generated clips and the fake
//! `whisper-cli` in `tests/fixtures/bin`. Tests skip when ffmpeg is not installed.

mod common;
mod fields;
mod pipeline;
mod templates;
