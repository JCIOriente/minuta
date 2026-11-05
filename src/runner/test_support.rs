//! Helpers for the runner's test modules.

use std::fs;
use std::path::Path;

use crate::config::{Config, Overrides};
use crate::job::Ctx;

pub(super) fn config(env: &[(&str, &str)]) -> Config {
    let lookup = |key: &str| {
        env.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.to_string())
    };
    Config::resolve(Overrides::default(), &lookup, None).unwrap()
}

/// A job folder whose `probe.json` lists audio-only sources of `durations` seconds.
pub(super) fn probed_job(dir: &Path, durations: &[f64]) -> Ctx {
    let sources: Vec<crate::steps::SourceProbe> = durations
        .iter()
        .enumerate()
        .map(|(i, &duration)| crate::steps::SourceProbe {
            path: dir.join(format!("{i}.m4a")),
            duration,
            video: None,
            audio: crate::steps::probe::AudioStream {
                codec: "aac".into(),
                sample_rate: 48000,
                channels: 2,
            },
        })
        .collect();
    crate::job::write_atomic(&crate::job::layout::probe_file(dir), |tmp| {
        Ok(fs::write(tmp, serde_json::to_string(&sources).unwrap())?)
    })
    .unwrap();
    Ctx {
        dir: dir.to_path_buf(),
        sources: sources.into_iter().map(|s| s.path).collect(),
        source_digest: "d".into(),
    }
}
