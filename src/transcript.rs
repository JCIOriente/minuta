use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    pub language: String,
    pub segments: Vec<Segment>,
}

#[derive(Deserialize)]
struct WhisperOutput {
    result: WhisperResult,
    transcription: Vec<WhisperSegment>,
}

#[derive(Deserialize)]
struct WhisperResult {
    language: String,
}

#[derive(Deserialize)]
struct WhisperSegment {
    offsets: WhisperOffsets,
    text: String,
}

#[derive(Deserialize)]
struct WhisperOffsets {
    from: i64,
    to: i64,
}

/// Parse `whisper-cli -oj` output. Segments are made ascending and
/// non-overlapping: overlaps are clamped, inverted segments are dropped.
pub fn from_whisper_json(json: &str) -> Result<Transcript> {
    let out: WhisperOutput =
        serde_json::from_str(json).map_err(|e| Error::Parse(format!("whisper JSON: {e}")))?;
    let mut segments: Vec<Segment> = Vec::with_capacity(out.transcription.len());
    let mut prev_end = 0.0_f64;
    for s in out.transcription {
        if s.offsets.to < s.offsets.from {
            eprintln!(
                "warning: dropping segment with end before start ({} ms > {} ms)",
                s.offsets.from, s.offsets.to
            );
            continue;
        }
        let start = (s.offsets.from as f64 / 1000.0).max(prev_end);
        let end = (s.offsets.to as f64 / 1000.0).max(start);
        prev_end = end;
        segments.push(Segment {
            start,
            end,
            text: s.text,
        });
    }
    Ok(Transcript {
        language: out.result.language,
        segments,
    })
}

/// One trimmed segment per line, no timestamps.
pub fn to_txt(t: &Transcript) -> String {
    let mut out = String::new();
    for s in &t.segments {
        out.push_str(s.text.trim());
        out.push('\n');
    }
    out
}

fn srt_time(seconds: f64) -> String {
    let ms = (seconds * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

pub fn to_srt(t: &Transcript) -> String {
    t.segments
        .iter()
        .enumerate()
        .map(|(i, s)| {
            format!(
                "{}\n{} --> {}\n{}\n",
                i + 1,
                srt_time(s.start),
                srt_time(s.end),
                s.text.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
