//! Strongest interactive directory-mode test feasible without a full Kitty graphics terminal.
//!
//! Uses a Unix pseudo-terminal so `lot` can enter raw mode, open directory playlist mode,
//! accept keystrokes (search + quit), and exit cleanly.
//!
//! This does **not** verify Kitty frame presentation (no graphics capability in CI PTYs).
//! It does verify process startup, directory-mode entry, input handling, and clean exit.

#![cfg(unix)]

mod common;

use common::{temp_dir, write_valid};
use std::fs;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::io::AsRawFd;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Minimal openpty binding (avoids new crates).
#[repr(C)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

unsafe extern "C" {
    fn openpty(
        amaster: *mut libc::c_int,
        aslave: *mut libc::c_int,
        name: *mut libc::c_char,
        termp: *mut libc::c_void,
        winp: *mut Winsize,
    ) -> libc::c_int;
}

fn open_pty() -> (OwnedFd, OwnedFd) {
    let mut master = 0;
    let mut slave = 0;
    let mut win = Winsize {
        ws_row: 40,
        ws_col: 120,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let rc = unsafe {
        openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut win,
        )
    };
    assert_eq!(rc, 0, "openpty failed");
    unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) }
}

fn set_nonblocking(fd: i32) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        assert!(flags >= 0, "F_GETFL failed");
        let rc = libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        assert_eq!(rc, 0, "F_SETFL O_NONBLOCK failed");
    }
}

#[test]
fn directory_mode_opens_accepts_search_keys_and_quits_on_pty() {
    let root = temp_dir("pty-dir");
    write_valid(&root.join("alpha.json"));
    write_valid(&root.join("beta.json"));
    write_valid(&root.join("nested").join("gamma.json"));

    let bin = assert_cmd::cargo::cargo_bin("lot");
    let (master, slave) = open_pty();
    let slave_fd = slave.as_raw_fd();

    let slave_in = unsafe { OwnedFd::from_raw_fd(libc::dup(slave_fd)) };
    let slave_out = unsafe { OwnedFd::from_raw_fd(libc::dup(slave_fd)) };
    let slave_err = unsafe { OwnedFd::from_raw_fd(libc::dup(slave_fd)) };
    drop(slave);

    let mut child = Command::new(&bin)
        .arg(root.as_os_str())
        .stdin(Stdio::from(slave_in))
        .stdout(Stdio::from(slave_out))
        .stderr(Stdio::from(slave_err))
        .env("TERM", "xterm-256color")
        .spawn()
        .expect("spawn lot on pty");

    let master_fd = master.as_raw_fd();
    set_nonblocking(master_fd);
    let mut master_file = unsafe {
        let fd = master.into_raw_fd();
        std::fs::File::from_raw_fd(fd)
    };

    // Wait until we see alternate-screen / app output (directory mode is drawing).
    let boot_deadline = Instant::now() + Duration::from_secs(5);
    let mut output = Vec::new();
    let mut buf = [0_u8; 8192];
    let mut saw_output = false;
    while Instant::now() < boot_deadline {
        match master_file.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => {
                output.extend_from_slice(&buf[..n]);
                saw_output = true;
                // Enough UI traffic to know the TUI is alive.
                if output.len() > 200 {
                    break;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => panic!("pty read failed during boot: {err}"),
        }
        if child.try_wait().ok().flatten().is_some() {
            panic!(
                "lot exited during boot; output={}",
                String::from_utf8_lossy(&output)
            );
        }
    }
    assert!(
        saw_output,
        "directory mode produced no TUI output on the PTY"
    );

    // Interactive sequence with pacing so crossterm can poll each key.
    // '/' enter search, 'a' filter, then documented quit via Ctrl-C.
    // (Plain 'q' is unreliable after Esc on some PTY/crossterm combinations because
    // ESC starts multi-byte sequences; Ctrl-C is an official quit key in lot.)
    for &byte in b"/a" {
        master_file.write_all(&[byte]).expect("write key");
        master_file.flush().ok();
        thread::sleep(Duration::from_millis(100));
    }
    // Down arrow after filter still exercises navigation while searching.
    master_file.write_all(b"\x1b[B").expect("write down arrow");
    master_file.flush().ok();
    thread::sleep(Duration::from_millis(100));

    // Ctrl-C quit (KeyModifiers::CONTROL + 'c').
    master_file.write_all(&[0x03]).expect("write ctrl-c");
    master_file.flush().ok();

    let deadline = Instant::now() + Duration::from_secs(6);
    while Instant::now() < deadline {
        match master_file.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => output.extend_from_slice(&buf[..n]),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
        if let Some(status) = child.try_wait().expect("try_wait") {
            assert!(
                status.success(),
                "lot exited with {status:?}; output bytes={}",
                output.len()
            );
            // Sanity: directory UI drew something resembling the app frame.
            let text = String::from_utf8_lossy(&output);
            assert!(
                text.contains("lot") || text.contains("Search") || text.contains("file"),
                "expected directory playlist UI markers in PTY output"
            );
            let _ = fs::remove_dir_all(&root);
            return;
        }
        thread::sleep(Duration::from_millis(30));
    }

    let _ = child.kill();
    let status = child.wait().expect("wait");
    let text = String::from_utf8_lossy(&output);
    panic!(
        "directory-mode PTY session did not exit cleanly within timeout; status={status:?}; output bytes={}; tail:\n{}",
        output.len(),
        text.chars()
            .rev()
            .take(600)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    );
}
