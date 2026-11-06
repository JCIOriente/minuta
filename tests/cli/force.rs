use assert_cmd::assert::OutputAssertExt;

use crate::common::*;
use crate::fake_tools::*;

#[test]
fn force_step_reruns_it_and_later_steps() {
    let env = FakeEnv::new();
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
    let env = FakeEnv::new();
    env.minuta(&["run", "clase-4.mp4"]).assert().success();
    let before = env.calls().len();

    let out = env.minuta(&["run", "clase-4.mp4", "--force"]);
    out.clone().assert().success();
    assert_eq!(
        env.tools_called()[before..],
        ["ffprobe", "ffmpeg", "whisper-cli"]
    );
    assert_eq!(
        cached_steps(&stderr(&out)).len(),
        0,
        "stderr was {}",
        stderr(&out)
    );
}

#[test]
fn force_accepts_template_name() {
    let env = FakeEnv::new();
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
    assert_eq!(cached_steps(&err).len(), 5, "stderr was {err}");
    assert!(!err.contains("summary     cached"), "stderr was {err}");
}

#[test]
fn force_template_also_forces_its_dependents() {
    let env = FakeEnv::new();
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
    let env = FakeEnv::new();
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
    assert_eq!(cached_steps(&stderr(&out)).len(), 4, "{}", stderr(&out));
}

#[test]
fn force_unknown_is_usage() {
    let env = FakeEnv::new();
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
