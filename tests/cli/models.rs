use std::fs;

use assert_cmd::assert::OutputAssertExt;

use crate::common::*;
use crate::fake_tools::*;

#[test]
fn models_list_shows_catalog() {
    let env = FakeEnv::new();
    let dir = env.path("home/.minuta/models");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("ggml-tiny.bin"), "tiny").unwrap();
    let out = env.minuta(&["models", "list"]);
    out.clone().assert().success();
    assert_eq!(
        stdout(&out),
        format!("models in {}\n", dir.display())
            + "tiny           installed  0.0 MiB\n\
         base           available  141.1 MiB\n\
         small          available  465.0 MiB\n\
         medium         available  1.4 GiB\n\
         large-v3       available  2.9 GiB\n\
         silero-v5.1.2  available  0.8 MiB\n"
    );
}

#[test]
fn models_list_header_uses_env_models_dir() {
    let env = FakeEnv::new();
    let custom = env.path("custom-models");
    let out = env.run_cmd({
        let mut cmd = env.command(&["models", "list"]);
        cmd.env("MINUTA_MODELS_DIR", &custom);
        cmd
    });
    out.clone().assert().success();
    assert_eq!(
        stdout(&out).lines().next(),
        Some(format!("models in {}", custom.display()).as_str())
    );
}

#[test]
fn models_list_reports_unreadable_entry() {
    let env = FakeEnv::new();
    let dir = env.path("home/.minuta/models");
    fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("ggml-small.bin");
    std::os::unix::fs::symlink(&entry, &entry).unwrap();
    let loop_error = fs::metadata(&entry).unwrap_err();
    let out = env.minuta(&["models", "list"]);
    out.clone().assert().code(1).stdout("");
    assert_eq!(
        stderr(&out),
        format!("minuta: I/O error: {}: {loop_error}\n", entry.display())
    );
}

#[test]
fn models_pull_unknown_name_is_usage_error() {
    let env = FakeEnv::new();
    env.minuta(&["models", "pull", "huge"])
        .assert()
        .code(2)
        .stderr(
            "minuta: usage error: unknown model `huge`; expected one of \
             tiny, base, small, medium, large-v3, silero-v5.1.2\n",
        );
}
