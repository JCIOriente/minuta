use assert_cmd::Command;
use predicates::str::contains;

use crate::common::spawn_lock;

#[test]
fn version_prints_crate_version() {
    let _guard = spawn_lock();
    Command::cargo_bin("minuta")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains(env!("CARGO_PKG_VERSION")));
}
