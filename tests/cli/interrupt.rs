use std::fs;
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use assert_cmd::assert::OutputAssertExt;

use crate::fake_tools::fake_ollama::Reply;
use crate::fake_tools::*;

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
    assert!(job.join("assets/audio.wav.tmp").exists());

    interrupt(&env, child, &pid);
    assert_eq!(tmp_files(&job), Vec::<String>::new());
    assert!(!job.join("assets/audio.wav").exists());
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
    assert!(!job.join("assets/transcript.json").exists());
    assert_eq!(step_status(&job, "audio").as_deref(), Some("done"));
    for step in ["transcribe", "outputs"] {
        assert_eq!(step_status(&job, step).as_deref(), Some("stale"), "{step}");
    }
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
