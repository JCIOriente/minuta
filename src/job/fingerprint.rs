//! Fingerprints: SHA-256 digests of a step's inputs, parameters and tool versions.

use std::fmt::Write;
use std::fs::{self, File};
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};

use super::state::Source;
use crate::error::Result;

/// Hashes `parts` into a lowercase hex SHA-256 digest. Each part is length-prefixed, so
/// `["ab", "c"]` and `["a", "bc"]` differ.
pub fn fingerprint(parts: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
    hex(&hasher.finalize())
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Stats and hashes the source file at `path`.
pub fn source_record(path: &Path) -> Result<Source> {
    let meta = fs::metadata(path)?;
    let mtime = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    Ok(Source {
        path: path.to_path_buf(),
        size: meta.len(),
        mtime,
        hash: hash_file(path)?,
    })
}

fn hash_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    io::copy(&mut File::open(path)?, &mut hasher)?;
    Ok(hex(&hasher.finalize()))
}
