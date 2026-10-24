//! Fingerprints: SHA-256 digests of a step's inputs, parameters and tool versions.

use std::fmt::Write;

use sha2::{Digest, Sha256};

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
