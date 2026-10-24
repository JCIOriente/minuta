use serde::{Deserialize, Serialize};

use crate::error::Result;

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

pub fn from_whisper_json(_json: &str) -> Result<Transcript> {
    unimplemented!()
}

pub fn to_txt(_t: &Transcript) -> String {
    unimplemented!()
}

pub fn to_srt(_t: &Transcript) -> String {
    unimplemented!()
}
