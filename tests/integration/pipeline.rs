use std::fs;

use crate::common::*;

const ALL: [&str; 5] = ["probe", "join", "audio", "transcribe", "outputs"];

fn ffprobe_json(path: &std::path::Path) -> serde_json::Value {
    let out = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_streams", "-of", "json"])
        .arg(path)
        .output()
        .unwrap();
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn full_run_produces_artifacts() {
    require_ffmpeg!();
    let env = Env::new();
    env.clip("a.mp4", 440);
    env.clip("b.mp4", 660);
    let out = env.minuta(&["run", "a.mp4", "b.mp4", "--name", "both"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let job = env.job_dir("both");
    assert_eq!(stdout(&out), format!("{}\n", job.display()));

    for f in [
        "joined.mp4",
        "audio.wav",
        "transcript.json",
        "transcript.txt",
        "captions.srt",
    ] {
        assert!(job.join(f).is_file(), "missing {f}");
    }
    assert_eq!(tmp_files(&job), Vec::<String>::new());

    let wav = ffprobe_json(&job.join("audio.wav"));
    let stream = &wav["streams"][0];
    assert_eq!(stream["channels"], 1);
    assert_eq!(stream["sample_rate"], "16000");
    // Two 1.5 s sources joined.
    let joined = ffprobe_json(&job.join("joined.mp4"));
    let dur: f64 = joined["streams"][0]["duration"]
        .as_str()
        .or(joined["streams"][1]["duration"].as_str())
        .unwrap()
        .parse()
        .unwrap();
    assert!(dur > 2.5, "joined duration {dur}");

    let transcript: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(job.join("transcript.json")).unwrap()).unwrap();
    assert!(transcript["segments"]
        .as_array()
        .is_some_and(|s| !s.is_empty()));
    let txt = fs::read_to_string(job.join("transcript.txt")).unwrap();
    assert!(txt.contains("Buenos días a todos."), "{txt}");

    // SRT: numbered cues, `hh:mm:ss,mmm --> hh:mm:ss,mmm`, text, blank line.
    let srt = fs::read_to_string(job.join("captions.srt")).unwrap();
    let blocks: Vec<&str> = srt.trim_end().split("\n\n").collect();
    assert!(!blocks.is_empty());
    for (i, block) in blocks.iter().enumerate() {
        let lines: Vec<&str> = block.lines().collect();
        assert_eq!(lines[0], (i + 1).to_string(), "{block}");
        let (from, to) = lines[1].split_once(" --> ").expect(block);
        for t in [from, to] {
            let b = t.as_bytes();
            assert!(
                t.len() == 12 && b[2] == b':' && b[5] == b':' && b[8] == b',',
                "bad timestamp {t}"
            );
        }
        assert!(lines.len() >= 3 && !lines[2].trim().is_empty(), "{block}");
    }
    assert!(job_json(&job)["steps"]["outputs"]["status"] == "done");
}

#[test]
fn rerun_is_cached() {
    require_ffmpeg!();
    let env = Env::new();
    env.clip("a.m4a", 440);
    env.minuta(&["run", "a.m4a"]);
    assert_eq!(env.whisper_calls(), 1);

    let out = env.minuta(&["run", "a.m4a"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(cached_steps(&stderr(&out)), ALL);
    assert_eq!(env.whisper_calls(), 1, "a cached run must not call whisper");
}

#[test]
fn lang_change_reruns_transcribe_only() {
    require_ffmpeg!();
    let env = Env::new();
    env.clip("a.m4a", 440);
    env.minuta(&["run", "a.m4a", "--lang", "es"]);

    let out = env.minuta(&["run", "a.m4a", "--lang", "en"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(cached_steps(&stderr(&out)), ["probe", "join", "audio"]);
    assert_eq!(env.whisper_calls(), 2);
    let log = fs::read_to_string(env.path("whisper.log")).unwrap();
    assert!(log.lines().last().unwrap().contains("-l en"), "{log}");

    // `reason` is cleared once a step finishes, so it is only visible while stale: rerun with
    // another language and stop at transcribe.
    let pid_file = env.path("whisper.pid");
    let mut cmd = env.command(&["run", "a.m4a", "--lang", "fr"]);
    cmd.env("FAKE_WHISPER_HANG", &pid_file);
    let child = cmd.spawn().unwrap();
    wait_for(&pid_file);
    signal("-INT", &child);
    child.wait_with_output().unwrap();
    let steps = &job_json(&env.job_dir("a"))["steps"];
    assert_eq!(steps["transcribe"]["status"], "stale");
    assert_eq!(steps["transcribe"]["reason"], "fingerprint changed");
    assert_eq!(steps["outputs"]["status"], "stale");
    assert!(steps["outputs"]["reason"].is_string());
    for step in ["probe", "join", "audio"] {
        assert_eq!(steps[step]["status"], "done", "{step}");
        assert!(steps[step]["reason"].is_null(), "{step}");
    }
}

#[test]
fn source_change_reruns_all() {
    require_ffmpeg!();
    let env = Env::new();
    env.clip("a.m4a", 440);
    env.minuta(&["run", "a.m4a"]);

    env.clip("a.m4a", 880);
    let out = env.minuta(&["run", "a.m4a"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(cached_steps(&stderr(&out)), Vec::<String>::new());
    assert_eq!(env.whisper_calls(), 2);
    assert_eq!(
        job_json(&env.job_dir("a"))["steps"]["outputs"]["status"],
        "done"
    );
}

#[test]
fn killed_step_resumes() {
    require_ffmpeg!();
    for sig in ["-INT", "-KILL"] {
        let env = Env::new();
        env.clip("a.m4a", 440);
        let pid_file = env.path("whisper.pid");
        let mut cmd = env.command(&["run", "a.m4a"]);
        cmd.env("FAKE_WHISPER_HANG", &pid_file);
        let child = cmd.spawn().unwrap();
        wait_for(&pid_file);
        signal(sig, &child);
        let out = child.wait_with_output().unwrap();
        assert!(!out.status.success());
        // Do not leave the (possibly orphaned) fake whisper behind.
        let pid = fs::read_to_string(&pid_file).unwrap();
        let _ = std::process::Command::new("kill")
            .args(["-KILL", pid.trim()])
            .status();

        let job = env.job_dir("a");
        assert!(!job.join("transcript.json").exists(), "{sig}");

        let out = env.minuta(&["run", "a.m4a"]);
        assert!(out.status.success(), "{sig}: {}", stderr(&out));
        // Probe, join and audio finished before the kill; transcribe did not.
        assert_eq!(
            cached_steps(&stderr(&out)),
            ["probe", "join", "audio"],
            "{sig}"
        );
        for f in ["transcript.json", "transcript.txt", "captions.srt"] {
            assert!(job.join(f).is_file(), "{sig}: missing {f}");
        }
        assert_eq!(tmp_files(&job), Vec::<String>::new(), "{sig}");
    }
}

#[test]
fn hostile_filename_is_inert() {
    require_ffmpeg!();
    let env = Env::new();
    let name = "my \"quoted\" 'file' $(touch pwned) `touch pwned2`; touch pwned3.m4a";
    let clip = env.clip("seed.m4a", 440);
    fs::rename(clip, env.path(name)).unwrap();

    let out = env.minuta(&["run", name]);
    assert!(out.status.success(), "{}", stderr(&out));
    let job = std::path::PathBuf::from(stdout(&out).trim());
    assert!(job.join("captions.srt").is_file());

    fn find(dir: &std::path::Path, hits: &mut Vec<String>) {
        for e in fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("pwned") {
                hits.push(e.path().display().to_string());
            }
            if e.file_type().unwrap().is_dir() {
                find(&e.path(), hits);
            }
        }
    }
    let mut hits = Vec::new();
    find(env.tmp.path(), &mut hits);
    assert!(hits.is_empty(), "command injection created {hits:?}");
}

#[test]
fn name_conflict_errors() {
    require_ffmpeg!();
    let env = Env::new();
    env.clip("a.m4a", 440);
    env.clip("b.m4a", 660);
    assert!(env
        .minuta(&["run", "a.m4a", "--name", "talk"])
        .status
        .success());
    let state = fs::read_to_string(env.job_dir("talk").join("job.json")).unwrap();

    let out = env.minuta(&["run", "b.m4a", "--name", "talk"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("--force"), "{}", stderr(&out));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        fs::read_to_string(env.job_dir("talk").join("job.json")).unwrap(),
        state,
        "the existing job is untouched"
    );

    // A different --name is a separate job.
    let out = env.minuta(&["run", "b.m4a", "--name", "other"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        format!("{}\n", env.job_dir("other").display())
    );
    assert!(env.job_dir("talk").join("captions.srt").is_file());
}
