use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

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
}

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
