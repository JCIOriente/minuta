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
echo "ffmpeg $*" >> '{log}'
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

#[test]
fn interrupt_kills_child_cleans_up_and_exits_130() {
    let env = Env::new();
    let pid_file = env.path("ffmpeg.pid");
    // The audio step starts its output, then hangs.
    env.ffmpeg(&format!(
        "echo $$ > '{}.part'; /bin/mv '{0}.part' '{0}'\nexec /bin/sleep 30",
        pid_file.display()
    ));
    let mut cmd = env.command(&["run", "clase-4.mp4"]);
    let child = env.spawn(&mut cmd);

    let deadline = Instant::now() + Duration::from_secs(10);
    while !pid_file.exists() {
        assert!(Instant::now() < deadline, "ffmpeg never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    let ffmpeg_pid = fs::read_to_string(&pid_file).unwrap().trim().to_string();
    let job = env.job_dir("clase-4");
    assert!(job.join("audio.wav.tmp").exists());

    let started = Instant::now();
    let mut kill = Command::new("kill");
    kill.args(["-INT", &child.id().to_string()]);
    assert!(env.spawn(&mut kill).wait().unwrap().success());
    let out = child.wait_with_output().unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));

    out.clone().assert().code(130).stdout("");
    assert!(stderr(&out).contains("interrupted"), "{}", stderr(&out));
    let mut alive = Command::new("kill");
    alive.args(["-0", &ffmpeg_pid]).stderr(Stdio::null());
    assert!(
        !env.spawn(&mut alive).wait().unwrap().success(),
        "ffmpeg was not killed"
    );
    let leftovers: Vec<_> = fs::read_dir(&job)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "left {leftovers:?}");
    assert!(!job.join("audio.wav").exists());
    assert_eq!(step_status(&job, "join").as_deref(), Some("done"));
    for step in ["audio", "transcribe", "outputs"] {
        assert_eq!(step_status(&job, step).as_deref(), Some("stale"), "{step}");
    }
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
         ffprobe.bin = ffprobe (default)\n"
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
