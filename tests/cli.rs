#![cfg(unix)]

//! The `minuta` binary with fake `ffmpeg`, `ffprobe` and `whisper-cli` scripts on `PATH`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use assert_cmd::assert::OutputAssertExt;
use predicates::str::contains;

/// An address no test server ever binds (port 9, discard), so no real Ollama is reached.
const NO_OLLAMA: &str = "http://127.0.0.1:9";

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/whisper/sample.json"
);

/// Held while writing a script and while spawning a process. A fork in another test thread
/// that inherits a script still open for writing makes running it fail with ETXTBSY on
/// Linux; serializing both rules that out.
static SPAWN: Mutex<()> = Mutex::new(());

fn spawn_lock() -> MutexGuard<'static, ()> {
    SPAWN.lock().unwrap_or_else(|e| e.into_inner())
}

fn script(dir: &Path, name: &str, body: &str) {
    let _guard = spawn_lock();
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A temp dir with fake tools in `bin/`, a model, an empty config file and one source.
/// Every tool appends its non-version invocations to `calls.log`.
struct Env {
    tmp: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
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

    fn path(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }

    fn bin(&self) -> PathBuf {
        self.path("bin")
    }

    fn log(&self) -> String {
        self.path("calls.log").display().to_string()
    }

    fn ffprobe(&self, extra: &str) {
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
    fn ffmpeg(&self, extra: &str) {
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

    fn whisper(&self, extra: &str) {
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
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.path("calls.log"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    /// The invoked tool names, in order.
    fn tools_called(&self) -> Vec<String> {
        self.calls()
            .iter()
            .map(|c| c.split(' ').next().unwrap().to_string())
            .collect()
    }

    fn job_dir(&self, name: &str) -> PathBuf {
        fs::canonicalize(self.path("minuta-out").join(name)).unwrap()
    }

    fn command(&self, args: &[&str]) -> Command {
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

    fn spawn(&self, cmd: &mut Command) -> Child {
        let _guard = spawn_lock();
        cmd.spawn().unwrap()
    }

    fn run_cmd(&self, mut cmd: Command) -> Output {
        self.spawn(&mut cmd).wait_with_output().unwrap()
    }

    fn minuta(&self, args: &[&str]) -> Output {
        self.run_cmd(self.command(args))
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Step lines reporting `cached`.
fn cached_lines(stderr: &str) -> usize {
    stderr
        .lines()
        .filter(|l| l.starts_with('[') && l.ends_with("  cached"))
        .count()
}

fn step_status(job: &Path, step: &str) -> Option<String> {
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(job.join("job.json")).unwrap()).unwrap();
    json["steps"][step]["status"].as_str().map(String::from)
}

#[test]
fn usage_error_exits_2() {
    let env = Env::new();
    env.minuta(&[]).assert().code(2);
    env.minuta(&["bogus"]).assert().code(2);
    env.minuta(&["run"]).assert().code(2);
    env.minuta(&["run", "clase-4.mp4", "--frobnicate"])
        .assert()
        .code(2);
    env.minuta(&["models"]).assert().code(2);
    env.minuta(&["config", "nope"]).assert().code(2);
    assert!(env.calls().is_empty());
}

#[test]
fn unknown_force_step_is_usage_error() {
    let env = Env::new();
    env.minuta(&["run", "clase-4.mp4", "--force", "render"])
        .assert()
        .code(2)
        .stderr(contains("unknown step `render`"));
    assert!(!env.path("minuta-out").exists());
}

#[test]
fn missing_source_file_is_usage_error() {
    let env = Env::new();
    env.minuta(&["run", "clase-4.mp4", "nope.mp4"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(contains("nope.mp4"));
    assert!(!env.path("minuta-out").exists());
    assert!(env.calls().is_empty());
}

#[test]
fn stdout_is_only_job_path() {
    let env = Env::new();
    let out = env.minuta(&["run", "clase-4.mp4"]);
    out.clone().assert().success();
    let job = env.job_dir("clase-4");
    assert_eq!(stdout(&out), format!("{}\n", job.display()));
    for artifact in [
        "job.json",
        "probe.json",
        "joined.mp4",
        "audio.wav",
        "transcript.json",
        "transcript.txt",
        "captions.srt",
    ] {
        assert!(job.join(artifact).is_file(), "missing {artifact}");
    }
    assert_eq!(
        env.tools_called(),
        ["ffprobe", "ffmpeg", "whisper-cli"],
        "probe, audio and transcribe run their tool; a single source is linked"
    );
    // Progress, with a step counter and timing, goes to stderr.
    let err = stderr(&out);
    for (i, step) in ["probe", "join", "audio", "transcribe", "outputs"]
        .iter()
        .enumerate()
    {
        assert!(
            err.contains(&format!("[{}/5] {step}", i + 1)),
            "stderr was {err}"
        );
    }
    assert!(err.contains("language=es"), "stderr was {err}");
    assert!(
        err.contains("note: VAD disabled (no VAD model configured)"),
        "stderr was {err}"
    );
}

#[test]
fn name_flag_strips_media_extension() {
    let env = Env::new();
    let out = env.minuta(&["run", "clase-4.mp4", "-n", "leccion.mp4"]);
    out.clone().assert().success();
    assert_eq!(
        stdout(&out),
        format!("{}\n", env.job_dir("leccion").display())
    );
}

#[test]
fn second_run_all_cached() {
    let env = Env::new();
    env.minuta(&["run", "clase-4.mp4"]).assert().success();
    let calls = env.calls();

    let out = env.minuta(&["run", "clase-4.mp4"]);
    out.clone().assert().success();
    assert_eq!(env.calls(), calls, "a cached run executes no step");
    let err = stderr(&out);
    assert_eq!(cached_lines(&err), 5, "stderr was {err}");
    assert!(err.contains("[4/5] transcribe  cached"), "stderr was {err}");
    assert!(!err.contains("VAD disabled"), "stderr was {err}");
    assert_eq!(
        stdout(&out),
        format!("{}\n", env.job_dir("clase-4").display())
    );
}

#[test]
fn transcribe_behaves_like_run() {
    let env = Env::new();
    let out = env.minuta(&["transcribe", "clase-4.mp4"]);
    out.clone().assert().success();
    assert_eq!(
        stdout(&out),
        format!("{}\n", env.job_dir("clase-4").display())
    );
    assert!(env.job_dir("clase-4").join("captions.srt").is_file());
    // Same job, so `run` finds every step cached.
    let calls = env.calls();
    env.minuta(&["run", "clase-4.mp4"]).assert().success();
    assert_eq!(env.calls(), calls);
}

#[test]
fn force_step_reruns_it_and_later_steps() {
    let env = Env::new();
    env.minuta(&["run", "clase-4.mp4"]).assert().success();
    let before = env.calls().len();

    let out = env.minuta(&["run", "clase-4.mp4", "--force", "transcribe"]);
    out.clone().assert().success();
    assert_eq!(env.tools_called()[before..], ["whisper-cli"]);
    let err = stderr(&out);
    assert!(err.contains("[1/5] probe       cached"), "stderr was {err}");
    assert!(err.contains("[3/5] audio       cached"), "stderr was {err}");
    assert!(
        !err.contains("[4/5] transcribe  cached"),
        "stderr was {err}"
    );
    assert!(
        !err.contains("[5/5] outputs     cached"),
        "stderr was {err}"
    );
}

#[test]
fn force_without_step_reruns_everything() {
    let env = Env::new();
    env.minuta(&["run", "clase-4.mp4"]).assert().success();
    let before = env.calls().len();

    let out = env.minuta(&["run", "clase-4.mp4", "--force"]);
    out.clone().assert().success();
    assert_eq!(
        env.tools_called()[before..],
        ["ffprobe", "ffmpeg", "whisper-cli"]
    );
    assert_eq!(
        cached_lines(&stderr(&out)),
        0,
        "stderr was {}",
        stderr(&out)
    );
}

#[test]
fn preflight_before_first_step() {
    let env = Env::new();
    fs::remove_file(env.bin().join("whisper-cli")).unwrap();
    let out = env.minuta(&["run", "clase-4.mp4"]);
    out.clone().assert().code(1).stdout("");
    let err = stderr(&out);
    assert!(err.contains("whisper-cli"), "stderr was {err}");
    assert!(err.contains("brew install whisper-cpp"), "stderr was {err}");
    assert!(!err.contains("[1/5]"), "no step may start: {err}");
    assert!(env.calls().is_empty());
    assert!(!env.path("minuta-out").exists());
}

#[test]
fn preflight_lists_all_missing_at_once() {
    let env = Env::new();
    for tool in ["ffmpeg", "ffprobe", "whisper-cli"] {
        fs::remove_file(env.bin().join(tool)).unwrap();
    }
    let out = env.run_cmd({
        let mut cmd = env.command(&["run", "clase-4.mp4"]);
        cmd.env_remove("MINUTA_WHISPER_MODEL");
        cmd
    });
    out.clone().assert().code(1).stdout("");
    let err = stderr(&out);
    for item in ["ffmpeg", "ffprobe", "whisper-cli", "model"] {
        assert!(err.contains(&format!("  {item}: ")), "stderr was {err}");
    }
    assert!(err.contains("minuta models pull base"), "stderr was {err}");
}

#[test]
fn catalog_model_not_pulled_hints_pull() {
    let env = Env::new();
    let out = env.minuta(&["run", "clase-4.mp4", "--model", "small"]);
    out.clone().assert().code(1);
    assert!(
        stderr(&out).contains("model: run `minuta models pull small`"),
        "stderr was {}",
        stderr(&out)
    );
}

#[test]
fn quiet_suppresses_progress() {
    let env = Env::new();
    let out = env.minuta(&["run", "clase-4.mp4", "--quiet"]);
    out.clone().assert().success().stderr("");
    assert_eq!(
        stdout(&out),
        format!("{}\n", env.job_dir("clase-4").display())
    );
}

#[test]
fn quiet_still_prints_warnings() {
    let env = Env::new();
    let empty = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/whisper/empty.json"
    );
    env.whisper(&format!("/bin/cp '{empty}' \"$prefix.json\"; exit 0"));
    let out = env.minuta(&["run", "clase-4.mp4", "-q"]);
    out.clone().assert().success();
    assert_eq!(
        stderr(&out),
        "warning: no speech detected: the transcript is empty\n"
    );
}

#[test]
fn tool_failure_exits_1_and_marks_step_stale() {
    let env = Env::new();
    env.whisper("echo 'model file is corrupt' >&2; exit 3");
    let out = env.minuta(&["run", "clase-4.mp4"]);
    out.clone().assert().code(1).stdout("");
    assert!(stderr(&out).contains("model file is corrupt"));
    let job = env.job_dir("clase-4");
    assert_eq!(step_status(&job, "audio").as_deref(), Some("done"));
    assert_eq!(step_status(&job, "transcribe").as_deref(), Some("stale"));
    assert!(!job.join("transcript.json").exists());
}

/// Shell lines that record the tool's pid in `pid_file`, then hang.
fn hang(pid_file: &Path) -> String {
    format!(
        "echo $$ > '{}.part'; /bin/mv '{0}.part' '{0}'\nexec /bin/sleep 30",
        pid_file.display()
    )
}

/// Starts `minuta run clase-4.mp4` and waits until the tool writing `pid_file` hangs;
/// returns the run and the tool's pid.
fn start_hanging_run(env: &Env, pid_file: &Path) -> (Child, String) {
    let child = env.spawn(&mut env.command(&["run", "clase-4.mp4"]));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !pid_file.exists() {
        assert!(Instant::now() < deadline, "the tool never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    let pid = fs::read_to_string(pid_file).unwrap().trim().to_string();
    (child, pid)
}

/// Sends SIGINT to the run, waits for it and checks that it killed the tool.
fn interrupt(env: &Env, child: Child, tool_pid: &str) -> Output {
    let started = Instant::now();
    let mut kill = Command::new("kill");
    kill.args(["-INT", &child.id().to_string()]);
    assert!(env.spawn(&mut kill).wait().unwrap().success());
    let out = child.wait_with_output().unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));

    out.clone().assert().code(130).stdout("");
    assert!(stderr(&out).contains("interrupted"), "{}", stderr(&out));
    let mut alive = Command::new("kill");
    alive.args(["-0", tool_pid]).stderr(Stdio::null());
    assert!(
        !env.spawn(&mut alive).wait().unwrap().success(),
        "the tool was not killed"
    );
    out
}

fn tmp_files(job: &Path) -> Vec<String> {
    fs::read_dir(job)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".tmp"))
        .collect()
}

#[test]
fn interrupt_kills_child_cleans_up_and_exits_130() {
    let env = Env::new();
    let pid_file = env.path("ffmpeg.pid");
    // The audio step starts its output, then hangs.
    env.ffmpeg(&hang(&pid_file));
    let (child, pid) = start_hanging_run(&env, &pid_file);
    let job = env.job_dir("clase-4");
    assert!(job.join("audio.wav.tmp").exists());

    interrupt(&env, child, &pid);
    assert_eq!(tmp_files(&job), Vec::<String>::new());
    assert!(!job.join("audio.wav").exists());
    assert_eq!(step_status(&job, "join").as_deref(), Some("done"));
    for step in ["audio", "transcribe", "outputs"] {
        assert_eq!(step_status(&job, step).as_deref(), Some("stale"), "{step}");
    }
}

#[test]
fn interrupt_during_transcribe_exits_130_with_outputs_stale() {
    let env = Env::new();
    let pid_file = env.path("whisper.pid");
    env.whisper(&hang(&pid_file));
    let (child, pid) = start_hanging_run(&env, &pid_file);

    interrupt(&env, child, &pid);
    let job = env.job_dir("clase-4");
    assert_eq!(tmp_files(&job), Vec::<String>::new());
    assert!(!job.join("transcript.json").exists());
    assert_eq!(step_status(&job, "audio").as_deref(), Some("done"));
    for step in ["transcribe", "outputs"] {
        assert_eq!(step_status(&job, step).as_deref(), Some("stale"), "{step}");
    }
}

#[test]
fn failing_encoder_listing_leaves_no_job_folder() {
    let env = Env::new();
    script(
        &env.bin(),
        "ffmpeg",
        r#"if [ "$1" = "-version" ]; then echo "ffmpeg version 7.1-fake"; exit 0; fi
echo "cannot list encoders" >&2
exit 1"#,
    );
    let out = env.minuta(&["run", "clase-4.mp4"]);
    out.clone().assert().code(1).stdout("");
    assert!(
        stderr(&out).contains("cannot list encoders"),
        "{}",
        stderr(&out)
    );
    assert!(!env.path("minuta-out").exists());
}

#[test]
fn bare_binary_name_is_looked_up_on_path() {
    let env = Env::new();
    fs::rename(env.bin().join("ffmpeg"), env.bin().join("ffmpeg7")).unwrap();
    let out = env.run_cmd({
        let mut cmd = env.command(&["run", "clase-4.mp4"]);
        cmd.env("MINUTA_FFMPEG_BIN", "ffmpeg7");
        cmd
    });
    out.clone().assert().success();
    assert_eq!(env.tools_called(), ["ffprobe", "ffmpeg7", "whisper-cli"]);
}

#[test]
fn different_sources_without_force_is_refused() {
    let env = Env::new();
    fs::write(env.path("other.mp4"), "more media").unwrap();
    env.minuta(&["run", "clase-4.mp4"])
        .clone()
        .assert()
        .success();
    let job = env.job_dir("clase-4");
    let state = fs::read_to_string(job.join("job.json")).unwrap();
    let calls = env.calls();

    let out = env.minuta(&["run", "clase-4.mp4", "other.mp4"]);
    out.clone().assert().code(1).stdout("");
    assert!(stderr(&out).contains("--force"), "{}", stderr(&out));
    assert_eq!(env.calls(), calls);
    assert_eq!(fs::read_to_string(job.join("job.json")).unwrap(), state);
}

#[test]
fn different_sources_with_another_name_is_a_separate_job() {
    let env = Env::new();
    fs::write(env.path("other.mp4"), "more media").unwrap();
    env.minuta(&["run", "clase-4.mp4"])
        .clone()
        .assert()
        .success();
    let state = fs::read_to_string(env.job_dir("clase-4").join("job.json")).unwrap();

    let out = env.minuta(&["run", "clase-4.mp4", "other.mp4", "--name", "both"]);
    out.clone().assert().success();
    assert_eq!(stdout(&out), format!("{}\n", env.job_dir("both").display()));
    assert_eq!(
        fs::read_to_string(env.job_dir("clase-4").join("job.json")).unwrap(),
        state
    );

    // The same name for yet other sources is refused too: --name never replaces a job.
    env.minuta(&["run", "other.mp4", "--name", "both"])
        .clone()
        .assert()
        .code(1);
}

#[test]
fn different_sources_with_force_replace_the_job() {
    let env = Env::new();
    fs::write(env.path("other.mp4"), "more media").unwrap();
    env.minuta(&["run", "clase-4.mp4"])
        .clone()
        .assert()
        .success();

    let out = env.minuta(&["run", "clase-4.mp4", "other.mp4", "--force"]);
    out.clone().assert().success();
    let job = env.job_dir("clase-4");
    assert_eq!(stdout(&out), format!("{}\n", job.display()));
    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(job.join("job.json")).unwrap()).unwrap();
    assert_eq!(state["sources"].as_array().unwrap().len(), 2);
    assert_eq!(step_status(&job, "outputs").as_deref(), Some("done"));
}

#[test]
fn doctor_exit_code_when_missing() {
    let env = Env::new();
    let out = env.minuta(&["doctor"]);
    out.clone().assert().success();
    let report = stdout(&out);
    assert!(report.contains("ffmpeg version 7.1-fake"), "{report}");
    assert!(report.contains("ffprobe version 7.1-fake"), "{report}");
    assert!(report.contains("whisper-cli 1.7.2-fake"), "{report}");
    assert!(
        report.contains(&env.path("ggml-test.bin").display().to_string()),
        "{report}"
    );

    fs::remove_file(env.bin().join("ffprobe")).unwrap();
    let out = env.minuta(&["doctor"]);
    out.clone().assert().code(1);
    assert!(stdout(&out).contains("ffmpeg version 7.1-fake"));
    assert!(stdout(&out).contains("missing"), "{}", stdout(&out));
    assert!(stderr(&out).contains("ffprobe"), "{}", stderr(&out));
}

#[test]
fn config_path_prints_path_even_when_malformed() {
    let env = Env::new();
    fs::write(env.path("config.toml"), "out_dir = \n").unwrap();
    let out = env.minuta(&["config", "path"]);
    out.clone().assert().success();
    assert_eq!(
        stdout(&out),
        format!("{}\n", env.path("config.toml").display())
    );
    env.minuta(&["config", "show"])
        .assert()
        .code(1)
        .stderr(contains("line 1"));
}

#[test]
fn config_show_lists_values_and_sources() {
    let env = Env::new();
    fs::write(env.path("config.toml"), "[whisper]\nlang = \"es\"\n").unwrap();
    let out = env.run_cmd({
        let mut cmd = env.command(&["config", "show"]);
        cmd.env_remove("MINUTA_WHISPER_MODEL")
            .env("MINUTA_FFMPEG_BIN", "/opt/ffmpeg");
        cmd
    });
    out.clone().assert().success();
    assert_eq!(
        stdout(&out),
        "out_dir = ./minuta-out (default)\n\
         whisper.bin = whisper-cli (default)\n\
         whisper.model = (unset)\n\
         whisper.lang = es (file)\n\
         ffmpeg.bin = /opt/ffmpeg (env)\n\
         ffprobe.bin = ffprobe (default)\n\
         llm.default = ollama:gemma4:26b (default)\n\
         llm.output_lang = (unset)\n\
         llm.concurrency = (unset)\n\
         llm.retries = 3 (default)\n\
         llm.backoff_ms = 2000 (default)\n\
         llm.repairs = 2 (default)\n\
         ollama.url = http://127.0.0.1:11434 (default)\n\
         ollama.num_ctx = 16384 (default)\n\
         claude.bin = claude (default)\n"
    );
}

#[test]
fn models_list_shows_catalog() {
    let env = Env::new();
    let dir = env.path("home/.cache/minuta/models");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("ggml-tiny.bin"), "tiny").unwrap();
    let out = env.minuta(&["models", "list"]);
    out.clone().assert().success();
    let lines: Vec<String> = stdout(&out).lines().map(String::from).collect();
    assert_eq!(lines.len(), 5, "{lines:?}");
    assert!(lines[0].starts_with("tiny") && lines[0].contains("installed"));
    assert!(lines[1].starts_with("base") && lines[1].contains("available"));
    assert!(lines[4].starts_with("large-v3"));
}

#[test]
fn models_pull_unknown_name_is_usage_error() {
    let env = Env::new();
    env.minuta(&["models", "pull", "huge"])
        .assert()
        .code(2)
        .stderr(contains("tiny, base, small, medium, large-v3"));
}

// ---------------------------------------------------------------------------------------
// Templates and LLM backends. The fake Ollama answers chats; the fake `claude` fixture
// stands in for Claude Code.

#[path = "support/fake_ollama.rs"]
mod fake_ollama;

use fake_ollama::{FakeOllama, Reply};
use std::sync::Arc;

const FAKE_CLAUDE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bin/claude");

/// A fake Ollama serving the default model; every chat gets `reply`.
fn ollama(reply: &'static str) -> FakeOllama {
    ollama_with(Arc::new(move |_| Reply::chat(reply)))
}

fn ollama_with(responder: fake_ollama::Responder) -> FakeOllama {
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
    fn template(&self, dir: &str, name: &str, text: &str) {
        let dir = self.path(dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name}.md")), text).unwrap();
    }

    /// `command` pointed at `fake`, with zero retry backoff and the fake `claude`.
    fn llm_command(&self, args: &[&str], fake: &FakeOllama) -> Command {
        let mut cmd = self.command(args);
        cmd.env("MINUTA_OLLAMA_URL", fake.url())
            .env("MINUTA_LLM_BACKOFF_MS", "0")
            .env("MINUTA_CLAUDE_BIN", FAKE_CLAUDE)
            // The fake `claude` needs `cat`; the fake tools in `bin/` still come first.
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin().display()));
        cmd
    }

    fn minuta_llm(&self, args: &[&str], fake: &FakeOllama) -> Output {
        self.run_cmd(self.llm_command(args, fake))
    }
}

fn template_state(job: &Path, name: &str) -> serde_json::Value {
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(job.join("job.json")).unwrap()).unwrap();
    json["templates"][name].clone()
}

#[test]
fn run_accepts_repeated_templates() {
    let env = Env::new();
    env.template("templates", "a", "Write A.");
    env.template("templates", "b", "Write B.");
    let fake = ollama("generated text");
    let out = env.minuta_llm(&["run", "clase-4.mp4", "-t", "a", "--template", "b"], &fake);
    out.clone().assert().success();
    let job = env.job_dir("clase-4");
    assert_eq!(stdout(&out), format!("{}\n", job.display()));
    for name in ["a", "b"] {
        assert_eq!(
            fs::read_to_string(job.join(format!("{name}.md"))).unwrap(),
            "generated text\n",
            "{name}"
        );
        assert_eq!(template_state(&job, name)["status"], "done", "{name}");
    }
    assert_eq!(fake.chat_count(), 2);
}

#[test]
fn transcribe_rejects_template_flag() {
    let env = Env::new();
    env.minuta(&["transcribe", "clase-4.mp4", "-t", "summary"])
        .assert()
        .code(2);
    env.minuta(&["transcribe", "clase-4.mp4", "--template", "summary"])
        .assert()
        .code(2);
    env.minuta(&["transcribe", "clase-4.mp4", "--llm", "claude"])
        .assert()
        .code(2);
    env.minuta(&["transcribe", "clase-4.mp4", "--output-lang", "en"])
        .assert()
        .code(2);
    assert!(!env.path("minuta-out").exists());
}

#[test]
fn force_accepts_template_name() {
    let env = Env::new();
    let fake = ollama("summary text");
    env.minuta_llm(&["run", "clase-4.mp4", "-t", "summary"], &fake)
        .assert()
        .success();
    assert_eq!(fake.chat_count(), 1);
    let calls = env.calls();

    let out = env.minuta_llm(
        &["run", "clase-4.mp4", "-t", "summary", "--force", "summary"],
        &fake,
    );
    out.clone().assert().success();
    assert_eq!(
        fake.chat_count(),
        2,
        "a forced template skips the call cache"
    );
    assert_eq!(env.calls(), calls, "no pipeline step is forced");
    let err = stderr(&out);
    assert_eq!(cached_lines(&err), 5, "stderr was {err}");
    assert!(!err.contains("summary     cached"), "stderr was {err}");
}

#[test]
fn force_template_also_forces_its_dependents() {
    let env = Env::new();
    env.template("templates", "a", "Write A.");
    env.template("templates", "b", "---\ninput: a\n---\nWrite B.");
    env.template("templates", "c", "Write C.");
    let fake = ollama("text");
    let args = ["run", "clase-4.mp4", "-t", "b", "-t", "c"];
    env.minuta_llm(&args, &fake).assert().success();
    assert_eq!(fake.chat_count(), 3);

    let mut forced = args.to_vec();
    forced.extend(["--force", "a"]);
    let out = env.minuta_llm(&forced, &fake);
    out.clone().assert().success();
    assert_eq!(fake.chat_count(), 5, "a and b rerun, c stays cached");
    let err = stderr(&out);
    assert!(err.contains("[8/8] c           cached"), "stderr was {err}");
}

#[test]
fn force_step_also_forces_templates() {
    let env = Env::new();
    let fake = ollama("summary text");
    env.minuta_llm(&["run", "clase-4.mp4", "-t", "summary"], &fake)
        .assert()
        .success();
    let out = env.minuta_llm(
        &["run", "clase-4.mp4", "-t", "summary", "--force", "outputs"],
        &fake,
    );
    out.clone().assert().success();
    assert_eq!(fake.chat_count(), 2);
    assert_eq!(cached_lines(&stderr(&out)), 4, "{}", stderr(&out));
}

#[test]
fn force_unknown_is_usage() {
    let env = Env::new();
    let fake = ollama("unused");
    let out = env.minuta_llm(
        &["run", "clase-4.mp4", "-t", "summary", "--force", "nope"],
        &fake,
    );
    out.clone().assert().code(2).stdout("");
    assert!(stderr(&out).contains("`nope`"), "{}", stderr(&out));
    assert!(stderr(&out).contains("summary"), "{}", stderr(&out));
    assert!(!env.path("minuta-out").exists());
    assert!(
        fake.requests().is_empty(),
        "resolution comes before preflight"
    );
}

#[test]
fn unknown_template_exits_2_before_job_folder() {
    let env = Env::new();
    let fake = ollama("unused");
    let out = env.minuta_llm(&["run", "clase-4.mp4", "-t", "nope"], &fake);
    out.clone().assert().code(2).stdout("");
    let err = stderr(&out);
    assert!(err.contains("unknown template 'nope'"), "{err}");
    assert!(err.contains("summary"), "lists the available names: {err}");
    assert!(!env.path("minuta-out").exists());
    assert!(env.calls().is_empty());
    assert!(fake.requests().is_empty());
}

#[test]
fn resolution_problems_exit_2_before_job_folder() {
    let env = Env::new();
    env.template("templates", "x", "---\ninput: y\n---\nX.");
    env.template("templates", "y", "---\ninput: x\n---\nY.");
    env.template("templates", "bad", "---\nllm: gpt:4\n---\nBad.");
    let fake = ollama("unused");
    let cases: [(&[&str], &str); 5] = [
        (&["-t", "x"], "template cycle"),
        (&["-t", "summary", "--llm", "gpt:4"], "gpt:4"),
        (&["-t", "summary", "--output-lang", "xx"], "'xx'"),
        (&["--output-lang", "xx"], "'xx'"),
        (&["-t", "bad"], "gpt:4"),
    ];
    for (extra, expected) in cases {
        let mut args = vec!["run", "clase-4.mp4"];
        args.extend(extra);
        let out = env.minuta_llm(&args, &fake);
        assert_eq!(out.status.code(), Some(2), "{extra:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains(expected),
            "{extra:?}: {}",
            stderr(&out)
        );
    }
    assert!(!env.path("minuta-out").exists());
    assert!(fake.requests().is_empty());
}

#[test]
fn preflight_reports_backend_and_tools_together() {
    let env = Env::new();
    fs::remove_file(env.bin().join("whisper-cli")).unwrap();
    let out = env.run_cmd({
        let mut cmd = env.command(&["run", "clase-4.mp4", "-t", "summary"]);
        cmd.env("MINUTA_OLLAMA_URL", NO_OLLAMA);
        cmd
    });
    out.clone().assert().code(1).stdout("");
    let err = stderr(&out);
    assert!(err.contains("  whisper-cli: "), "{err}");
    assert!(
        err.contains("  ollama: start Ollama (`ollama serve`)"),
        "{err}"
    );
    assert!(!env.path("minuta-out").exists());
}

#[test]
fn preflight_reports_missing_ollama_model_and_claude() {
    let env = Env::new();
    env.template("templates", "acta", "---\nllm: claude\n---\nMinutes.");
    let fake = FakeOllama::start(
        &["other:7b"],
        serde_json::json!({}),
        Arc::new(|_| Reply::chat("unused")),
    );
    let out = env.run_cmd({
        let mut cmd = env.llm_command(
            &["run", "clase-4.mp4", "-t", "summary", "-t", "acta"],
            &fake,
        );
        cmd.env(
            "MINUTA_CLAUDE_BIN",
            env.path("no-claude").display().to_string(),
        );
        cmd
    });
    out.clone().assert().code(1);
    let err = stderr(&out);
    assert!(
        err.contains("  ollama:gemma4:26b: ollama pull gemma4:26b"),
        "{err}"
    );
    assert!(err.contains("  claude: "), "{err}");
    assert!(!env.path("minuta-out").exists());
}

#[test]
fn run_without_templates_contacts_no_backend() {
    let env = Env::new();
    let fake = ollama("unused");
    env.minuta_llm(&["run", "clase-4.mp4"], &fake)
        .assert()
        .success();
    assert!(fake.requests().is_empty());
    assert!(!env.job_dir("clase-4").join("llm-cache").exists());
}

#[test]
fn second_template_run_is_cached() {
    let env = Env::new();
    let fake = ollama("summary text");
    env.minuta_llm(&["run", "clase-4.mp4", "-t", "summary"], &fake)
        .assert()
        .success();
    let out = env.minuta_llm(&["run", "clase-4.mp4", "-t", "summary"], &fake);
    out.clone().assert().success();
    assert_eq!(fake.chat_count(), 1);
    let err = stderr(&out);
    assert_eq!(cached_lines(&err), 6, "stderr was {err}");
    assert!(err.contains("[6/6] summary     cached"), "stderr was {err}");
    assert!(err.contains("(0 run, 6 cached)"), "stderr was {err}");
}

#[test]
fn progress_template_line_format() {
    let env = Env::new();
    let fake = ollama("summary text");
    let out = env.minuta_llm(&["run", "clase-4.mp4", "-t", "summary"], &fake);
    out.clone().assert().success();
    let err = stderr(&out);
    let line = err
        .lines()
        .find(|l| l.starts_with("[6/6] summary"))
        .unwrap_or_else(|| panic!("no template line in {err}"));
    let rest = line
        .strip_prefix("[6/6] summary     ollama:gemma4:26b  1 call  ")
        .unwrap_or_else(|| panic!("line was {line:?}"));
    assert!(rest.ends_with('s'), "time: {rest:?}");
}

#[test]
fn progress_total_includes_templates() {
    let env = Env::new();
    env.template("templates", "a", "Write A.");
    env.template("templates", "b", "---\ninput: a\n---\nWrite B.");
    let fake = ollama("text");
    let out = env.minuta_llm(&["run", "clase-4.mp4", "-t", "b"], &fake);
    out.clone().assert().success();
    let err = stderr(&out);
    for line in ["[1/7] probe", "[5/7] outputs", "[6/7] a    ", "[7/7] b    "] {
        assert!(err.contains(line), "missing {line:?} in {err}");
    }
    assert!(err.contains("(7 run, 0 cached)"), "{err}");
}

#[test]
fn off_machine_note_printed_once() {
    let env = Env::new();
    env.template("templates", "acta", "---\nllm: claude\n---\nMinutes.");
    let fake = ollama("local text");
    let out = env.minuta_llm(
        &[
            "run",
            "clase-4.mp4",
            "-t",
            "acta",
            "-t",
            "summary",
            "-t",
            "acta",
        ],
        &fake,
    );
    out.clone().assert().success();
    let err = stderr(&out);
    let note = "note: acta sends its input to claude (Anthropic)";
    assert_eq!(err.matches(note).count(), 1, "{err}");
    assert!(!err.contains("summary sends"), "{err}");
    let job = env.job_dir("clase-4");
    assert_eq!(
        fs::read_to_string(job.join("acta.md")).unwrap(),
        "fake claude reply\n"
    );
    assert_eq!(fake.chat_count(), 1, "only summary uses Ollama");
}

#[test]
fn llm_flag_overrides_the_backend() {
    let env = Env::new();
    let fake = ollama("unused");
    let out = env.minuta_llm(
        &["run", "clase-4.mp4", "-t", "summary", "--llm", "claude"],
        &fake,
    );
    out.clone().assert().success();
    assert!(fake.requests().is_empty(), "no Ollama check for claude");
    assert!(stderr(&out).contains("note: summary sends its input to claude (Anthropic)"));
    assert!(stderr(&out).contains("[6/6] summary     claude  1 call  "));
}

#[test]
fn output_lang_flag_reaches_the_prompt() {
    let env = Env::new();
    let fake = ollama("text");
    env.minuta_llm(
        &["run", "clase-4.mp4", "-t", "summary", "--output-lang", "en"],
        &fake,
    )
    .assert()
    .success();
    let chat = fake
        .requests()
        .into_iter()
        .find(|r| r.path == "/api/chat")
        .unwrap();
    let system = chat.body["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(system.contains("in English"), "{system}");
}

#[test]
fn template_failure_stops_the_run() {
    let env = Env::new();
    env.template("templates", "a", "Write A.");
    let fake = ollama_with(Arc::new(|_| Reply::error(400, "bad request")));
    let out = env.minuta_llm(&["run", "clase-4.mp4", "-t", "a", "-t", "summary"], &fake);
    out.clone().assert().code(1).stdout("");
    assert!(stderr(&out).contains("bad request"), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("[6/7] a           failed"),
        "{}",
        stderr(&out)
    );
    let job = env.job_dir("clase-4");
    assert_eq!(template_state(&job, "a")["status"], "stale");
    assert_eq!(template_state(&job, "summary"), serde_json::Value::Null);
    assert_eq!(
        fake.chat_count(),
        1,
        "a 400 is not retried and summary never runs"
    );
}

#[test]
fn interrupt_during_template_exits_130() {
    let env = Env::new();
    let fake = ollama_with(Arc::new(|_| Reply::Hang));
    let child = env.spawn(&mut env.llm_command(&["run", "clase-4.mp4", "-t", "summary"], &fake));
    let deadline = Instant::now() + Duration::from_secs(10);
    while fake.chat_count() == 0 {
        assert!(Instant::now() < deadline, "the chat never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut kill = Command::new("kill");
    kill.args(["-INT", &child.id().to_string()]);
    assert!(env.spawn(&mut kill).wait().unwrap().success());
    let out = child.wait_with_output().unwrap();
    out.clone().assert().code(130).stdout("");
    assert!(stderr(&out).contains("interrupted"), "{}", stderr(&out));
    let job = env.job_dir("clase-4");
    assert!(!job.join("summary.md").exists());
    assert_eq!(template_state(&job, "summary")["status"], "stale");
    assert_eq!(step_status(&job, "outputs").as_deref(), Some("done"));
}

#[test]
fn templates_list_shows_layers() {
    let env = Env::new();
    env.template(
        "home/.config/minuta/templates",
        "acta",
        "---\ndescription: Formal minutes\nllm: claude\n---\nMinutes.",
    );
    env.template("templates", "notes", "---\ninput: acta\n---\nNotes.");
    env.template("templates", "summary", "My own summary.");
    env.template("templates", "broken", "---\nnope: 1\n---\nX.");
    let out = env.minuta(&["templates", "list"]);
    out.clone().assert().success();
    let text = stdout(&out);
    let rows: Vec<Vec<&str>> = text
        .lines()
        .map(|l| {
            l.split("  ")
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .collect()
        })
        .collect();
    assert_eq!(
        rows[0],
        ["NAME", "LAYER", "INPUT", "LLM", "DESCRIPTION"],
        "{text}"
    );
    assert!(
        rows.contains(&vec![
            "acta",
            "user",
            "transcript",
            "claude",
            "Formal minutes"
        ]),
        "{text}"
    );
    assert!(
        rows.contains(&vec!["notes", "project", "acta", "(default)"]),
        "{text}"
    );
    assert!(
        rows.contains(&vec!["summary", "project", "transcript", "(default)"]),
        "{text}"
    );
    let broken = env.path("templates/broken.md").display().to_string();
    let invalid = text
        .lines()
        .find(|l| l.contains(&broken))
        .unwrap_or_else(|| panic!("no invalid entry in {text}"));
    assert!(invalid.contains("nope"), "{invalid}");
}

#[test]
fn templates_list_shows_built_in() {
    let env = Env::new();
    let out = env.minuta(&["templates", "list"]);
    out.clone().assert().success();
    let text = stdout(&out);
    let summary = text.lines().find(|l| l.starts_with("summary")).unwrap();
    let cols: Vec<&str> = summary
        .split("  ")
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .collect();
    assert_eq!(
        cols,
        [
            "summary",
            "built-in",
            "transcript",
            "(default)",
            "Short summary of the recording"
        ]
    );
}

#[test]
fn templates_show_prints_winning_file() {
    let env = Env::new();
    let out = env.minuta(&["templates", "show", "summary"]);
    out.clone().assert().success();
    assert_eq!(stderr(&out), "built-in\n");
    assert!(
        stdout(&out).starts_with("---\ndescription: Short summary"),
        "{}",
        stdout(&out)
    );

    env.template("templates", "summary", "Mine.\n");
    let out = env.minuta(&["templates", "show", "summary"]);
    out.clone().assert().success();
    assert_eq!(
        stderr(&out),
        // The project layer is `./templates`, found through the canonical current directory.
        format!(
            "{}\n",
            fs::canonicalize(env.path("templates/summary.md"))
                .unwrap()
                .display()
        )
    );
    assert_eq!(stdout(&out), "Mine.\n");
}

#[test]
fn templates_show_unknown_exits_2() {
    let env = Env::new();
    env.minuta(&["templates", "show", "nope"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(contains("unknown template 'nope'"));
}

#[test]
fn doctor_reports_llm_without_affecting_exit_code() {
    let env = Env::new();
    let fake = ollama("unused");
    let out = env.minuta_llm(&["doctor"], &fake);
    out.clone().assert().success();
    assert!(
        stdout(&out).contains("llm          ollama:gemma4:26b (ok)"),
        "{}",
        stdout(&out)
    );

    let out = env.run_cmd({
        let mut cmd = env.llm_command(&["doctor"], &fake);
        cmd.env("MINUTA_LLM", "ollama:missing:1b");
        cmd
    });
    out.clone().assert().success();
    assert!(
        stdout(&out)
            .contains("llm          ollama:missing:1b (model missing: ollama pull missing:1b)"),
        "{}",
        stdout(&out)
    );

    let out = env.run_cmd({
        let mut cmd = env.command(&["doctor"]);
        cmd.env("MINUTA_OLLAMA_URL", NO_OLLAMA);
        cmd
    });
    out.clone().assert().success();
    assert!(
        stdout(&out).contains("llm          ollama:gemma4:26b (not reachable: "),
        "{}",
        stdout(&out)
    );
}

/// Every char that could move the cursor or start an escape sequence.
fn control_chars(text: &str) -> Vec<char> {
    text.chars()
        .filter(|c| c.is_control() && *c != '\n')
        .collect()
}

#[test]
fn templates_list_neutralizes_control_characters() {
    let env = Env::new();
    env.template(
        "templates",
        "evil",
        "---\ndescription: \"ok\\e[2K\\rfake\\nsummary  built-in\"\n---\nX.",
    );
    // An ignored file whose name, shown in the path and the error, has a control character.
    env.template("templates", "bro\x1b[31mken", "X.");
    let out = env.minuta(&["templates", "list"]);
    out.clone().assert().success();
    let text = stdout(&out);
    assert_eq!(control_chars(&text), Vec::<char>::new(), "{text:?}");
    let evil: Vec<&str> = text.lines().filter(|l| l.contains("fake")).collect();
    assert_eq!(evil.len(), 1, "{text}");
    assert!(evil[0].starts_with("evil "), "{text}");
    let broken: Vec<&str> = text.lines().filter(|l| l.contains("bro?[31mken")).collect();
    assert_eq!(broken.len(), 1, "{text}");
    assert!(broken[0].starts_with("invalid "), "{text}");
    assert_eq!(
        text.lines().filter(|l| l.starts_with("summary")).count(),
        1,
        "no fake row: {text}"
    );
}

#[test]
fn doctor_reports_ollama_show_failure_as_not_reachable() {
    let env = Env::new();
    let fake =
        FakeOllama::start_with_show(&["gemma4:26b"], None, Arc::new(|_| Reply::chat("unused")));
    let out = env.minuta_llm(&["doctor"], &fake);
    out.clone().assert().success();
    let report = stdout(&out);
    assert!(
        report.contains("llm          ollama:gemma4:26b (not reachable: "),
        "{report}"
    );
    assert!(!report.contains("model missing"), "{report}");
}

/// A template whose `output` value carries an SGR sequence and an OSC title change.
const EVIL_OUTPUT: &str = "---\noutput: \"x\\e[31mRED\\e]0;pwned\\a\"\n---\nX.";

#[test]
fn template_errors_reach_stderr_without_control_characters() {
    let env = Env::new();
    env.template("templates", "acta", EVIL_OUTPUT);
    env.template("templates", "notes", "---\ninput: acta\n---\nNotes.");
    let fake = ollama("unused");
    let cases: [&[&str]; 3] = [
        &["run", "clase-4.mp4", "-t", "acta"],
        &["run", "clase-4.mp4", "-t", "notes"],
        &["templates", "show", "acta"],
    ];
    for args in cases {
        let out = env.minuta_llm(args, &fake);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
        let err = stderr(&out);
        assert!(
            err.contains("RED"),
            "{args:?}: the error names the value: {err:?}"
        );
        assert_eq!(control_chars(&err), Vec::<char>::new(), "{args:?}: {err:?}");
    }
    assert!(!env.path("minuta-out").exists());
}
