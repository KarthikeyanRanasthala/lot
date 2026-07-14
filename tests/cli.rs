use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn exposes_headless_options_in_help() {
    Command::cargo_bin("lot")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--headless"))
        .stdout(predicate::str::contains("--animation-id"));
}

#[test]
fn explains_that_headless_rendering_is_not_available() {
    Command::cargo_bin("lot")
        .unwrap()
        .args([
            "animation.json",
            "--headless",
            "--width",
            "100",
            "--height",
            "100",
            "--fps",
            "30",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "headless frame output is not available yet",
        ));
}
