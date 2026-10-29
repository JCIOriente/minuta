use crate::error::{Error, Result};
use directories::BaseDirs;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where whisper model files are downloaded from.
pub const WHISPER_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// Where the Silero VAD model files for `whisper-cli --vad-model` are downloaded from.
pub const VAD_BASE_URL: &str = "https://huggingface.co/ggml-org/whisper-vad/resolve/main";

/// What a catalog entry is for: `whisper.model` or `whisper.vad_model`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Whisper,
    Vad,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec<'a> {
    pub name: &'a str,
    pub kind: ModelKind,
    /// The file name, both on disk and at `base_url`.
    pub file: &'a str,
    pub base_url: &'a str,
    pub sha256: &'a str,
    pub size: u64,
}

impl ModelSpec<'_> {
    /// Where [`pull`] downloads the file from.
    pub fn url(&self) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), self.file)
    }
}

// SHA-256 and size values come from the Git LFS pointer files of the upstream repository,
// e.g. https://huggingface.co/ggerganov/whisper.cpp/raw/main/ggml-small.bin or
// https://huggingface.co/ggml-org/whisper-vad/raw/main/ggml-silero-v5.1.2.bin
// (`oid sha256:...` and `size`).
pub const CATALOG: &[ModelSpec<'static>] = &[
    ModelSpec {
        name: "tiny",
        file: "ggml-tiny.bin",
        kind: ModelKind::Whisper,
        base_url: WHISPER_BASE_URL,
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        size: 77_691_713,
    },
    ModelSpec {
        name: "base",
        file: "ggml-base.bin",
        kind: ModelKind::Whisper,
        base_url: WHISPER_BASE_URL,
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        size: 147_951_465,
    },
    ModelSpec {
        name: "small",
        file: "ggml-small.bin",
        kind: ModelKind::Whisper,
        base_url: WHISPER_BASE_URL,
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        size: 487_601_967,
    },
    ModelSpec {
        name: "medium",
        file: "ggml-medium.bin",
        kind: ModelKind::Whisper,
        base_url: WHISPER_BASE_URL,
        sha256: "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
        size: 1_533_763_059,
    },
    ModelSpec {
        name: "large-v3",
        file: "ggml-large-v3.bin",
        kind: ModelKind::Whisper,
        base_url: WHISPER_BASE_URL,
        sha256: "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2",
        size: 3_095_033_483,
    },
    // The VAD model for `whisper.vad_model`, not a whisper model.
    ModelSpec {
        name: "silero-v5.1.2",
        file: "ggml-silero-v5.1.2.bin",
        kind: ModelKind::Vad,
        base_url: VAD_BASE_URL,
        sha256: "29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf",
        size: 885_098,
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

/// How long to wait for a connection and for each read; there is no overall timeout because
/// large models take a long time on slow links.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// `~/.minuta/models`: the folder for models that work with whisper.
pub fn default_dir() -> Result<PathBuf> {
    let base = BaseDirs::new()
        .ok_or_else(|| Error::Config("cannot determine the home directory".into()))?;
    Ok(default_dir_for(base.home_dir()))
}

/// [`default_dir`] for the given home directory.
pub fn default_dir_for(home: &Path) -> PathBuf {
    home.join(".minuta").join("models")
}

/// Where `spec` lives under `dir`: whisper models sit flat (as in whisper.cpp), VAD models
/// in `vad/`.
fn path_in(spec: &ModelSpec, dir: &Path) -> PathBuf {
    match spec.kind {
        ModelKind::Whisper => dir.join(spec.file),
        ModelKind::Vad => dir.join("vad").join(spec.file),
    }
}

/// A catalog name resolves into `dir`, even when a file of that name exists in the working
/// directory; any other existing file is used as is.
pub fn resolve(name_or_path: &str, dir: &Path) -> Result<PathBuf> {
    if let Some(m) = CATALOG.iter().find(|m| m.name == name_or_path) {
        return Ok(path_in(m, dir));
    }
    let as_path = Path::new(name_or_path);
    if as_path.is_file() {
        return Ok(as_path.to_path_buf());
    }
    Err(Error::Usage(format!(
        "unknown model '{name_or_path}': not a file and not one of {}",
        CATALOG
            .iter()
            .map(|m| m.name)
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

/// Every catalog entry and whether it is in `dir`. A missing file, or a path that is not a
/// file, is "not installed"; any other metadata error is returned, naming the path.
pub fn list(dir: &Path) -> Result<Vec<ModelInfo>> {
    CATALOG
        .iter()
        .map(|m| {
            let path = path_in(m, dir);
            let on_disk = match fs::metadata(&path) {
                Ok(md) if md.is_file() => Some(md),
                Ok(_) => None,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => {
                    return Err(Error::Io(std::io::Error::new(
                        e.kind(),
                        format!("{}: {e}", path.display()),
                    )))
                }
            };
            Ok(ModelInfo {
                name: m.name.to_string(),
                installed: on_disk.is_some(),
                size: on_disk.map_or(m.size, |md| md.len()),
            })
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
/// Reads at most `expected_size + 1` bytes and rejects any other length.
fn download(name: &str, url: &str, part: &Path, expected_size: u64) -> Result<String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .build();
    let response = agent.get(url).call().map_err(|e| {
        let reason = match &e {
            ureq::Error::Status(code, _) => format!("HTTP status {code}"),
            ureq::Error::Transport(_) => e.to_string(),
        };
        Error::Io(std::io::Error::other(format!(
            "download of {url} failed: {reason}"
        )))
    })?;
    let mut reader = response.into_reader().take(expected_size + 1);
    let mut out = File::create(part)?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
    }
    out.sync_all()?;
    if total != expected_size {
        return Err(Error::Job(format!("size mismatch for {name}")));
    }
    Ok(hex_digest(hasher))
}

/// Downloads `spec` into `dir` unless a file with the pinned checksum is already there.
/// The bytes come from [`ModelSpec::url`] and go to a temp file that is renamed into place
/// only after the checksum matches.
pub fn pull(spec: &ModelSpec, dir: &Path) -> Result<Pulled> {
    let target = path_in(spec, dir);
    if target.is_file() {
        if file_sha256(&target)? == spec.sha256 {
            return Ok(Pulled::AlreadyPresent);
        }
        fs::remove_file(&target)?;
    }
    // `path_in` always puts the target inside `dir`, so it has a parent.
    let parent = target.parent().expect("a model path always has a parent");
    fs::create_dir_all(parent)?;
    let part = target.with_file_name(format!(".{}.part", spec.file));
    let url = spec.url();
    let result = download(spec.name, &url, &part, spec.size).and_then(|digest| {
        if digest == spec.sha256 {
            fs::rename(&part, &target)?;
            Ok(Pulled::Downloaded)
        } else {
            Err(Error::Job(format!("checksum mismatch for {}", spec.name)))
        }
    });
    if result.is_err() {
        // Best effort: the original error is what the caller needs to see.
        let _ = fs::remove_file(&part);
    }
    result
}
