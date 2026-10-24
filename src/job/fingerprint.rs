//! Fingerprints: SHA-256 digests of a step's inputs, parameters and tool versions.

use std::fmt::Write;
use std::fs::{self, File};
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};

use super::state::Source;
use super::{Ctx, Step};
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

/// The fingerprint of `step` in `ctx`: covers the step name, the content of every source,
/// the step's parameters (which include its tool version) and the fingerprint of the
/// previous step, if any.
pub fn step_fingerprint<S: Step>(step: &S, ctx: &Ctx, previous: Option<&str>) -> String {
    fingerprint(&[
        S::NAME.as_str().as_bytes(),
        ctx.source_digest.as_bytes(),
        &step.params(ctx),
        previous.unwrap_or_default().as_bytes(),
    ])
}

/// Digest of the sources' contents, in order.
pub(super) fn source_digest(sources: &[Source]) -> String {
    let hashes: Vec<&[u8]> = sources.iter().map(|s| s.hash.as_bytes()).collect();
    fingerprint(&hashes)
}

/// Stats the source file at `path`. Its content is hashed only when the size or mtime differ
/// from `previous`, the record of the same path from the last run.
pub fn source_record(path: &Path, previous: Option<&Source>) -> Result<Source> {
    let meta = fs::metadata(path)?;
    let size = meta.len();
    let mtime = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let hash = match previous {
        Some(p) if p.path == path && p.size == size && p.mtime == mtime => p.hash.clone(),
        _ => hash_file(path)?,
    };
    Ok(Source {
        path: path.to_path_buf(),
        size,
        mtime,
        hash,
    })
}

fn hash_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    io::copy(&mut File::open(path)?, &mut hasher)?;
    Ok(hex(&hasher.finalize()))
}
