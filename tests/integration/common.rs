use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

#[path = "../support/fake_ollama.rs"]
pub mod fake_ollama;

use fake_ollama::{FakeOllama, Request};

pub const FAKE_WHISPER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/bin/whisper-cli"
);

pub fn have_ffmpeg() -> bool {
    ["ffmpeg", "ffprobe"].iter().all(|t| {
        Command::new(t)
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

/// Returns early from the test when ffmpeg or ffprobe is missing.
macro_rules! require_ffmpeg {
    () => {
        if !$crate::common::have_ffmpeg() {
            assert!(
                std::env::var("MINUTA_REQUIRE_FFMPEG").as_deref() != Ok("1"),
                "MINUTA_REQUIRE_FFMPEG=1 but ffmpeg and ffprobe are not on PATH"
            );
            eprintln!("skipping: ffmpeg and ffprobe are not on PATH");
            return;
        }
    };
}
pub(crate) use require_ffmpeg;

pub const FAKE_CLAUDE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bin/claude");
pub const CLASS_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/whisper/class.json"
);

/// A hermetic temp dir: own HOME, an empty config, the output folder and the sources.
pub struct Env {
    pub tmp: tempfile::TempDir,
}

impl Env {
    pub fn new() -> Env {
        let env = Env {
            tmp: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(env.path("home")).unwrap();
        fs::write(env.path("config.toml"), "").unwrap();
        fs::write(env.path("ggml-test.bin"), "model").unwrap();
        env
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }

    /// Generates a clip (video+tone for `.mp4`, tone only otherwise) at `name`. `hz` makes
    /// clips differ in content.
    pub fn clip(&self, name: &str, hz: u32) -> PathBuf {
        let path = self.path(name);
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-nostdin", "-v", "error", "-y"]).args([
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency={hz}:duration=1.5"),
        ]);
        if name.ends_with(".mp4") {
            cmd.args([
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=160x120:rate=10:duration=1.5",
            ])
            .args(["-c:v", "mpeg4", "-c:a", "aac", "-shortest"]);
        }
        let status = cmd.arg(&path).status().unwrap();
        assert!(status.success(), "ffmpeg could not generate {name}");
        path
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(assert_cmd::cargo::cargo_bin("minuta"));
        let path = std::env::var_os("PATH").unwrap_or_default();
        cmd.args(args)
            .current_dir(self.tmp.path())
            .env_clear()
            .env("PATH", path)
            .env("HOME", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("home/.config"))
            .env("XDG_CACHE_HOME", self.path("home/.cache"))
            .env("MINUTA_CONFIG", self.path("config.toml"))
            .env("MINUTA_WHISPER_BIN", FAKE_WHISPER)
            .env("MINUTA_WHISPER_MODEL", self.path("ggml-test.bin"))
            .env("FAKE_WHISPER_LOG", self.path("whisper.log"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    pub fn minuta(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    /// Runs minuta and asserts success (for setup steps).
    pub fn run_ok(&self, args: &[&str]) -> Output {
        let out = self.minuta(args);
        assert!(out.status.success(), "{args:?} failed: {}", stderr(&out));
        out
    }

    /// Number of whisper-cli invocations so far.
    pub fn whisper_calls(&self) -> usize {
        fs::read_to_string(self.path("whisper.log"))
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }

    pub fn job_dir(&self, name: &str) -> PathBuf {
        fs::canonicalize(self.path("minuta-out").join(name)).unwrap()
    }

    /// Writes `<dir>/<name>.md`, where `dir` is relative to the temp dir.
    pub fn template(&self, dir: &str, name: &str, text: &str) {
        let dir = self.path(dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.md")), text).unwrap();
    }

    /// `command` pointed at `url`, with zero retry backoff and the fake `claude`.
    pub fn llm_command(&self, args: &[&str], url: &str) -> Command {
        let mut cmd = self.command(args);
        cmd.env("MINUTA_OLLAMA_URL", url)
            .env("MINUTA_LLM_BACKOFF_MS", "0")
            .env("MINUTA_CLAUDE_BIN", FAKE_CLAUDE);
        cmd
    }

    pub fn run_llm(&self, args: &[&str], fake: &FakeOllama) -> Output {
        self.llm_command(args, &fake.url()).output().unwrap()
    }
}

pub fn env_with_clip() -> Env {
    let env = Env::new();
    env.clip("a.mp4", 440);
    env
}

pub fn ollama_with(responder: fake_ollama::Responder) -> FakeOllama {
    FakeOllama::start(
        &["gemma4:26b"],
        serde_json::json!({
            "model_info": {"gemma4.context_length": 32768},
            "capabilities": ["completion"]
        }),
        responder,
    )
}

pub fn message(req: &Request, index: usize) -> &str {
    req.body["messages"][index]["content"].as_str().unwrap()
}

pub fn chats(fake: &FakeOllama) -> Vec<Request> {
    fake.requests()
        .into_iter()
        .filter(|r| r.path == "/api/chat")
        .collect()
}

/// Whether the request asks for structured output (final and repair calls only).
pub fn has_format(req: &Request) -> bool {
    !req.body["format"].is_null()
}

/// Whether the request is a call of the `chapters` template (its schema asks for `chapters`).
pub fn is_chapters(req: &Request) -> bool {
    !req.body["format"]["properties"]["chapters"].is_null()
}

pub fn is_repair(req: &Request) -> bool {
    message(req, 1).starts_with("Previous answer:")
}

pub fn chapters_answer(chapters: &[(&str, &str)]) -> String {
    let list: Vec<_> = chapters
        .iter()
        .map(|(start, title)| serde_json::json!({"start": start, "title": title}))
        .collect();
    serde_json::json!({ "chapters": list }).to_string()
}

/// A valid chapters answer for `class.json`: every start is a segment start of the transcript.
pub fn valid_chapters() -> String {
    chapters_answer(&[
        ("00:00:00", "Introducción"),
        ("00:00:32", "La propiedad"),
        ("00:01:47", "Movimientos y copias"),
        ("00:03:25", "Préstamos"),
        ("00:04:40", "Ejercicios"),
    ])
}

/// A valid answer for the built-in `youtube` template.
pub fn valid_youtube() -> String {
    serde_json::json!({
        "title": "Rust ownership",
        "body": "A class about ownership.",
        "tags": ["rust", "ownership"],
        "thumbnail_text": "Ownership",
    })
    .to_string()
}

/// The `youtube.md` of a run that answered `valid_youtube` and `valid_chapters`.
pub const VALID_YOUTUBE_MD: &str = "Rust ownership\n\nA class about ownership.\n\n0:00 Introducción\n0:32 La propiedad\n1:47 Movimientos y copias\n3:25 Préstamos\n4:40 Ejercicios\n\nrust, ownership\n\nOwnership\n";

pub fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

pub fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn job_json(job: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(job.join("job.json")).unwrap()).unwrap()
}

/// Steps whose progress line in `stderr` says `cached`.
pub fn cached_steps(stderr: &str) -> Vec<String> {
    stderr
        .lines()
        .filter(|l| l.starts_with('[') && l.ends_with("  cached"))
        .map(|l| l.split_whitespace().nth(1).unwrap().to_string())
        .collect()
}

pub fn tmp_files(job: &Path) -> Vec<String> {
    fs::read_dir(job)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".tmp"))
        .collect()
}

pub fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "{} never appeared",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub fn signal(sig: &str, child: &Child) {
    let ok = Command::new("kill")
        .args([sig, &child.id().to_string()])
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

/// Whether `pid` is a live process.
pub fn pid_alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

pub fn kill_pid(pid: &str) {
    let _ = Command::new("kill").args(["-KILL", pid]).status();
}

/// Starts `minuta args` with the fake whisper hanging; returns the run and whisper's pid.
pub fn start_hanging(env: &Env, args: &[&str]) -> (Child, String) {
    let pid_file = env.path("whisper.pid");
    let mut cmd = env.command(args);
    cmd.env("FAKE_WHISPER_HANG", &pid_file);
    let child = cmd.spawn().unwrap();
    wait_for(&pid_file);
    let pid = fs::read_to_string(&pid_file).unwrap().trim().to_string();
    (child, pid)
}
