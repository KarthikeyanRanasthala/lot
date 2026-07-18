//! Shared helpers for integration tests that exercise production playlist code.
//!
//! Each integration binary only pulls a subset of these helpers, so unused items are expected.

#![allow(dead_code)]

use lot::playlist::{Playlist, PlaylistEvent};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub fn fixture_json_bytes() -> &'static [u8] {
    include_bytes!("../../fixtures/two_frames.json")
}

pub fn fixture_json_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/two_frames.json")
}

pub fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("lot-itest-{label}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn write_valid(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, fixture_json_bytes()).unwrap();
}

pub fn write_corrupt(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, b"{not-valid-json").unwrap();
}

/// Apply a production watcher event the same way the TUI does (generation gate + replace).
pub fn apply_playlist_event(
    playlist: &mut Playlist,
    event: PlaylistEvent,
    last_generation: &mut u64,
) -> bool {
    match event {
        PlaylistEvent::ScanComplete { generation, paths } => {
            assert!(
                generation >= *last_generation || *last_generation == 0,
                "watcher generation went backwards: {generation} < {last_generation}"
            );
            if generation < *last_generation {
                return false;
            }
            *last_generation = generation;
            playlist.replace_entries(paths);
            true
        }
        PlaylistEvent::Error { message } => {
            // Non-fatal: keep last good playlist, surface for debugging.
            eprintln!("playlist watch error: {message}");
            false
        }
    }
}

pub fn recv_scan(rx: &Receiver<PlaylistEvent>, timeout: Duration) -> Option<(u64, Vec<PathBuf>)> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(PlaylistEvent::ScanComplete { generation, paths }) => {
                return Some((generation, paths));
            }
            Ok(PlaylistEvent::Error { .. }) => continue,
            Err(RecvTimeoutError::Timeout) => break,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    None
}

/// Wait until a scan satisfies `predicate`, applying each accepted event to `playlist`.
///
/// The predicate receives `(&Playlist, last_generation)`.
pub fn wait_for_scan(
    rx: &Receiver<PlaylistEvent>,
    playlist: &mut Playlist,
    last_generation: &mut u64,
    timeout: Duration,
    mut predicate: impl FnMut(&Playlist, u64) -> bool,
) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(remaining.min(Duration::from_millis(200))) {
            Ok(event) => {
                if apply_playlist_event(playlist, event, last_generation)
                    && predicate(playlist, *last_generation)
                {
                    return true;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if predicate(playlist, *last_generation) {
                    return true;
                }
            }
            Err(RecvTimeoutError::Disconnected) => return false,
        }
    }
    predicate(playlist, *last_generation)
}

pub fn assert_playlist_invariants(playlist: &Playlist) {
    let paths: Vec<_> = playlist.entries().iter().map(|e| &e.path).collect();
    let unique: std::collections::BTreeSet<_> = paths.iter().collect();
    assert_eq!(
        unique.len(),
        paths.len(),
        "duplicate playlist paths: {paths:?}"
    );

    for path in &paths {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        assert!(
            ext == "json" || ext == "lottie",
            "non-animation entry: {}",
            path.display()
        );
    }

    if let Some(selected) = playlist.selected_index() {
        assert!(
            selected < playlist.entries().len(),
            "selected index {selected} out of range {}",
            playlist.entries().len()
        );
        assert!(
            playlist.filtered_position().is_some(),
            "selected entry is not visible under filter {:?}",
            playlist.filter()
        );
    } else {
        assert!(
            playlist.filtered_len() == 0,
            "empty selection with non-empty filtered list"
        );
    }
}

pub fn playlist_path_set(playlist: &Playlist) -> std::collections::BTreeSet<PathBuf> {
    playlist.entries().iter().map(|e| e.path.clone()).collect()
}
