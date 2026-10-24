use crate::error::{Error, Result};
use directories::BaseDirs;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Where model files are downloaded from.
pub const DEFAULT_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec<'a> {
    pub name: &'a str,
    pub sha256: &'a str,
    pub size: u64,
}

// SHA-256 and size values come from the Git LFS pointer files of the upstream repository,
// e.g. https://huggingface.co/ggerganov/whisper.cpp/raw/main/ggml-small.bin
// (`oid sha256:...` and `size`). Files are named `ggml-<name>.bin`.
pub const CATALOG: &[ModelSpec<'static>] = &[
    ModelSpec {
        name: "tiny",
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        size: 77_691_713,
    },
    ModelSpec {
        name: "base",
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        size: 147_951_465,
    },
    ModelSpec {
        name: "small",
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        size: 487_601_967,
    },
    ModelSpec {
        name: "medium",
        sha256: "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
        size: 1_533_763_059,
    },
    ModelSpec {
        name: "large-v3",
        sha256: "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2",
        size: 3_095_033_483,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub name: String,
    pub installed: bool,
    /// Size on disk when installed, otherwise the download size.
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pulled {
    AlreadyPresent,
    Downloaded,
}

/// `~/.cache/minuta/models`.
pub fn default_dir() -> Result<PathBuf> {
    let base = BaseDirs::new()
        .ok_or_else(|| Error::Config("cannot determine the home directory".into()))?;
    Ok(base.home_dir().join(".cache").join("minuta").join("models"))
}

fn file_name(name: &str) -> String {
    format!("ggml-{name}.bin")
}

/// An existing path is used as is; a catalog name resolves into `dir`.
pub fn resolve(name_or_path: &str, dir: &Path) -> Result<PathBuf> {
    let as_path = Path::new(name_or_path);
    if as_path.exists() {
        return Ok(as_path.to_path_buf());
    }
    match CATALOG.iter().find(|m| m.name == name_or_path) {
        Some(m) => Ok(dir.join(file_name(m.name))),
        None => Err(Error::Usage(format!(
            "unknown model '{name_or_path}': not a file and not one of {}",
            CATALOG
                .iter()
                .map(|m| m.name)
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

pub fn list(dir: &Path) -> Vec<ModelInfo> {
    CATALOG
        .iter()
        .map(|m| {
            let on_disk = fs::metadata(dir.join(file_name(m.name)))
                .ok()
                .filter(|md| md.is_file());
            ModelInfo {
                name: m.name.to_string(),
                installed: on_disk.is_some(),
                size: on_disk.map_or(m.size, |md| md.len()),
            }
        })
        .collect()
}

fn hex_digest(hasher: Sha256) -> String {
    hasher.finalize().iter().fold(String::new(), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

fn file_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok(hex_digest(hasher));
        }
        hasher.update(&buf[..n]);
    }
}

/// Streams the model into `part`, returning the SHA-256 of what was written.
fn download(url: &str, part: &Path) -> Result<String> {
    let response = ureq::get(url).call().map_err(|e| {
        Error::Io(std::io::Error::other(format!(
            "download of {url} failed: {e}"
        )))
    })?;
    let mut reader = response.into_reader();
    let mut out = File::create(part)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
    }
    out.sync_all()?;
    Ok(hex_digest(hasher))
}

/// Downloads `spec` into `dir` unless a file with the pinned checksum is already there.
/// The bytes go to a temp file that is renamed into place only after the checksum matches.
pub fn pull(spec: &ModelSpec, dir: &Path, base_url: &str) -> Result<Pulled> {
    let target = dir.join(file_name(spec.name));
    if target.is_file() {
        if file_sha256(&target)? == spec.sha256 {
            return Ok(Pulled::AlreadyPresent);
        }
        fs::remove_file(&target)?;
    }
    fs::create_dir_all(dir)?;
    let part = dir.join(format!(".{}.part", file_name(spec.name)));
    let url = format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        file_name(spec.name)
    );
    let result = download(&url, &part).and_then(|digest| {
        if digest == spec.sha256 {
            fs::rename(&part, &target)?;
            Ok(Pulled::Downloaded)
        } else {
            Err(Error::Job(format!("checksum mismatch for {}", spec.name)))
        }
    });
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}
