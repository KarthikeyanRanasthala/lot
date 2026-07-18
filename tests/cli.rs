use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;
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

/// Serve `body` once over HTTP/1.1 on a random localhost port.
fn serve_once(body: Vec<u8>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut buf = [0_u8; 4096];
        let _ = stream.read(&mut buf);
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(&body);
        let _ = stream.flush();
    });
    (format!("http://{addr}/two_frames.json"), handle)
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
    Command::cargo_bin("lot")
        .unwrap()
        .arg("not-a-valid-local-file.lottie")
        .assert()
        .failure();
}

#[test]
fn dead_url_fails_as_download_not_playlist() {
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

#[test]
fn successful_url_load_from_local_http_server() {
    let body = fs::read(fixture_json()).expect("fixture");
    let expected_len = 5 * 4 * 3 * 4;
    let (url, server) = serve_once(body);

    Command::cargo_bin("lot")
        .unwrap()
        .args([
            &url,
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
        .stdout(predicate::function(move |output: &[u8]| {
            output.len() == expected_len
        }));

    let _ = server.join();
}
