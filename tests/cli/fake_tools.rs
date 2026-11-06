//! The `minuta` binary with fake `ffmpeg`, `ffprobe` and `whisper-cli` scripts on `PATH`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;

use fake_ollama::{FakeOllama, Reply};

use crate::common::spawn_lock;

/// An address no test server ever binds (port 9, discard), so no real Ollama is reached.
pub const NO_OLLAMA: &str = "http://127.0.0.1:9";

pub const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/whisper/sample.json"
);

/// Writes an executable `#!/bin/sh` script, under `common::spawn_lock` so no process forked
/// meanwhile inherits it open for writing.
pub fn script(dir: &Path, name: &str, body: &str) {
    let _guard = spawn_lock();
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A temp dir with fake tools in `bin/`, a model, an empty config file and one source.
/// Every tool appends its non-version invocations to `calls.log`.
pub struct Env {
    tmp: tempfile::TempDir,
}

impl Env {
    pub fn new() -> Env {
        let env = Env {
            tmp: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(env.bin()).unwrap();
        fs::create_dir_all(env.path("home")).unwrap();
        fs::write(env.path("config.toml"), "").unwrap();
        fs::write(env.path("ggml-test.bin"), "model").unwrap();
        fs::write(env.path("clase-4.mp4"), "media").unwrap();
        env.ffprobe("");
        env.ffmpeg("");
        env.whisper("");
        env
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }

    pub fn bin(&self) -> PathBuf {
        self.path("bin")
    }

    pub fn log(&self) -> String {
        self.path("calls.log").display().to_string()
    }

    pub fn ffprobe(&self, extra: &str) {
        script(
            &self.bin(),
            "ffprobe",
            &format!(
                r#"if [ "$1" = "-version" ]; then echo "ffprobe version 7.1-fake"; exit 0; fi
echo "ffprobe $*" >> '{log}'
{extra}
echo '{{"streams":[{{"codec_type":"audio","codec_name":"aac","sample_rate":"48000","channels":1}}],"format":{{"duration":"2.0"}}}}'"#,
                log = self.log()
            ),
        );
    }

    /// `extra` runs after the output file (the last argument) has been started.
    pub fn ffmpeg(&self, extra: &str) {
        script(
            &self.bin(),
            "ffmpeg",
            &format!(
                r#"if [ "$1" = "-version" ]; then echo "ffmpeg version 7.1-fake"; exit 0; fi
case " $* " in *" -encoders "*) echo " V....D libx264  H.264"; exit 0;; esac
echo "${{0##*/}} $*" >> '{log}'
for last; do :; done
printf 'RIFF' > "$last"
echo "size=1kB time=00:00:01.00 bitrate=1kbits/s speed=10x" >&2
{extra}"#,
                log = self.log()
            ),
        );
    }

    pub fn whisper(&self, extra: &str) {
        script(
            &self.bin(),
            "whisper-cli",
            &format!(
                r#"if [ "$1" = "--version" ]; then echo "whisper-cli 1.7.2-fake"; exit 0; fi
echo "whisper-cli $*" >> '{log}'
prefix=
while [ $# -gt 0 ]; do
  if [ "$1" = "-of" ]; then prefix=$2; fi
  shift
done
{extra}
echo "[00:00:00.000 --> 00:00:03.500]   Buenos dias a todos."
/bin/cp '{FIXTURE}' "$prefix.json""#,
                log = self.log()
            ),
        );
    }

    /// Work invocations so far (version probes and the encoder listing are not logged).
    pub fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.path("calls.log"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    /// The invoked tool names, in order.
    pub fn tools_called(&self) -> Vec<String> {
        self.calls()
            .iter()
            .map(|c| c.split(' ').next().unwrap().to_string())
            .collect()
    }

    pub fn job_dir(&self, name: &str) -> PathBuf {
        fs::canonicalize(self.path("minuta-out").join(name)).unwrap()
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(assert_cmd::cargo::cargo_bin("minuta"));
        cmd.args(args)
            .current_dir(self.tmp.path())
            .env_clear()
            .env("PATH", self.bin())
            .env("HOME", self.path("home"))
            .env("MINUTA_CONFIG", self.path("config.toml"))
            .env("MINUTA_WHISPER_MODEL", self.path("ggml-test.bin"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Never probe a real local Ollama. `config` is left alone: `config show` lists the
        // default URL.
        if args.first() != Some(&"config") {
            cmd.env("MINUTA_OLLAMA_URL", NO_OLLAMA);
        }
        cmd
    }

    pub fn spawn(&self, cmd: &mut Command) -> Child {
        crate::common::spawn(cmd)
    }

    pub fn run_cmd(&self, mut cmd: Command) -> Output {
        self.spawn(&mut cmd).wait_with_output().unwrap()
    }

    pub fn minuta(&self, args: &[&str]) -> Output {
        self.run_cmd(self.command(args))
    }
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

pub fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Step lines reporting `cached`.
pub fn cached_lines(stderr: &str) -> usize {
    stderr
        .lines()
        .filter(|l| l.starts_with('[') && l.ends_with("  cached"))
        .count()
}

pub fn step_status(job: &Path, step: &str) -> Option<String> {
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(minuta::job::layout::state_file(job)).unwrap())
            .unwrap();
    json["steps"][step]["status"].as_str().map(String::from)
}

// ---------------------------------------------------------------------------------------
// Templates and LLM backends. The fake Ollama answers chats; the fake `claude` fixture
// stands in for Claude Code.

pub use crate::common::fake_ollama;

pub const FAKE_CLAUDE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bin/claude");
pub const FAKE_CODEX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bin/codex");

/// A fake Ollama serving the default model; every chat gets `reply`.
pub fn ollama(reply: &'static str) -> FakeOllama {
    ollama_with(Arc::new(move |_| Reply::chat(reply)))
}

pub fn ollama_with(responder: fake_ollama::Responder) -> FakeOllama {
    FakeOllama::start(
        &["gemma4:26b"],
        serde_json::json!({
            "model_info": {"gemma3.context_length": 32768},
            "capabilities": ["completion"]
        }),
        responder,
    )
}

impl Env {
    /// Writes `<dir>/<name>.md`, where `dir` is relative to the temp dir.
    pub fn template(&self, dir: &str, name: &str, text: &str) {
        let dir = self.path(dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.md")), text).unwrap();
    }

    /// `command` pointed at `fake`, with zero retry backoff and the fake `claude`.
    pub fn llm_command(&self, args: &[&str], fake: &FakeOllama) -> Command {
        let mut cmd = self.command(args);
        cmd.env("MINUTA_OLLAMA_URL", fake.url())
            .env("MINUTA_LLM_BACKOFF_MS", "0")
            .env("MINUTA_CLAUDE_BIN", FAKE_CLAUDE)
            .env("MINUTA_CODEX_BIN", FAKE_CODEX)
            // The fake `claude` and `codex` need `cat`; the fake tools in `bin/` still come first.
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin().display()));
        cmd
    }

    pub fn minuta_llm(&self, args: &[&str], fake: &FakeOllama) -> Output {
        self.run_cmd(self.llm_command(args, fake))
    }
}

pub fn template_state(job: &Path, name: &str) -> serde_json::Value {
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(minuta::job::layout::state_file(job)).unwrap())
            .unwrap();
    json["templates"][name].clone()
}
