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
fn writes_raw_rgba_frames_in_headless_mode() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/two_frames.json");

    Command::cargo_bin("lot")
        .unwrap()
        .args([
            fixture,
            "--headless",
            "--width",
            "4",
            "--height",
            "3",
            "--fps",
            "5",
        ])
        .assert()
        .success()
        .stdout(predicate::function(|output: &[u8]| {
            output.len() == 5 * 4 * 3 * 4
        }));
}
