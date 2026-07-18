use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("lot-cli-{label}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture_json() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/two_frames.json")
}

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
fn help_mentions_directory_input() {
    Command::cargo_bin("lot")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("directory"));
}

#[test]
fn writes_raw_rgba_frames_in_headless_mode() {
    let fixture = fixture_json();

    Command::cargo_bin("lot")
        .unwrap()
        .args([
            fixture.to_str().unwrap(),
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

#[test]
fn headless_rejects_directory_input() {
    let dir = temp_dir("headless-dir");
    fs::copy(fixture_json(), dir.join("a.json")).unwrap();

    Command::cargo_bin("lot")
        .unwrap()
        .args([
            dir.to_str().unwrap(),
            "--headless",
            "--width",
            "4",
            "--height",
            "3",
            "--fps",
            "5",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(
            predicate::str::contains("directory playlist mode")
                .or(predicate::str::contains("interactive terminal")),
        );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn headless_accepts_file_inside_directory_tree() {
    let dir = temp_dir("headless-file");
    let file = dir.join("a.json");
    fs::copy(fixture_json(), &file).unwrap();

    Command::cargo_bin("lot")
        .unwrap()
        .args([
            file.to_str().unwrap(),
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

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn missing_local_path_fails_with_context() {
    let missing = std::env::temp_dir().join(format!(
        "lot-missing-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    Command::cargo_bin("lot")
        .unwrap()
        .arg(missing.to_str().unwrap())
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not exist").or(predicate::str::contains("No such")));
}

#[test]
fn empty_directory_is_rejected_in_headless_mode() {
    let dir = temp_dir("empty");

    Command::cargo_bin("lot")
        .unwrap()
        .args([
            dir.to_str().unwrap(),
            "--headless",
            "--width",
            "4",
            "--height",
            "3",
            "--fps",
            "5",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("directory"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn invalid_url_scheme_path_is_treated_as_local() {
    // A non-http(s) string is classified as a local path and fails when missing.
    Command::cargo_bin("lot")
        .unwrap()
        .arg("not-a-valid-local-file.lottie")
        .assert()
        .failure();
}

#[test]
fn url_input_is_not_classified_as_local_directory() {
    // Should attempt a download (and fail without hanging on playlist mode).
    Command::cargo_bin("lot")
        .unwrap()
        .args([
            "https://127.0.0.1:1/definitely-not-listening.lottie",
            "--headless",
            "--width",
            "4",
            "--height",
            "3",
            "--fps",
            "5",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("download")
                .or(predicate::str::contains("request"))
                .or(predicate::str::contains("Connection"))
                .or(predicate::str::contains("error")),
        );
}
