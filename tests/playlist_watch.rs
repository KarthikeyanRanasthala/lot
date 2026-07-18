//! Deterministic filesystem-watcher tests against production
//! `spawn_directory_watcher` + `Playlist` + `discover_animations`.

mod common;

use common::{
    assert_playlist_invariants, playlist_path_set, recv_scan, temp_dir, wait_for_scan,
    write_corrupt, write_valid,
};
use lot::playlist::{Playlist, WatcherSession, discover_animations, spawn_directory_watcher};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

fn boot_playlist(
    root: PathBuf,
) -> (
    Playlist,
    u64,
    Receiver<lot::playlist::PlaylistEvent>,
    WatcherSession,
    PathBuf,
) {
    let root = root.canonicalize().unwrap();
    let (rx, session) = spawn_directory_watcher(root.clone()).unwrap();
    let mut playlist = Playlist::new(root.clone());
    let (generation, paths) = recv_scan(&rx, Duration::from_secs(8)).expect("initial scan");
    assert!(generation >= 1);
    playlist.replace_entries(paths);
    assert_playlist_invariants(&playlist);
    (playlist, generation, rx, session, root)
}

/// Keep re-applying `kick` until the watcher delivers a playlist state matching `pred`.
fn wait_until(
    rx: &Receiver<lot::playlist::PlaylistEvent>,
    playlist: &mut Playlist,
    last_generation: &mut u64,
    timeout: Duration,
    mut kick: impl FnMut(),
    mut pred: impl FnMut(&Playlist, u64) -> bool,
) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        kick();
        if wait_for_scan(
            rx,
            playlist,
            last_generation,
            Duration::from_millis(600),
            |p, generation| pred(p, generation),
        ) {
            return true;
        }
    }
    pred(playlist, *last_generation)
}

fn path_ends_with(entry_rel: &Path, name: &str) -> bool {
    entry_rel.file_name().is_some_and(|n| n == name) || entry_rel.ends_with(name)
}

#[test]
fn watcher_add_modify_rename_delete() {
    let tmp = temp_dir("watch-crud");
    write_valid(&tmp.join("a.json"));
    write_valid(&tmp.join("b.json"));

    let (mut playlist, mut last_gen, rx, session, root) = boot_playlist(tmp);
    assert_eq!(playlist.len(), 2);

    // --- Add ---
    let nested = root.join("nested").join("c.json");
    assert!(
        wait_until(
            &rx,
            &mut playlist,
            &mut last_gen,
            Duration::from_secs(15),
            || write_valid(&nested),
            |p, _| {
                p.len() >= 3
                    && p.entries()
                        .iter()
                        .any(|e| path_ends_with(&e.relative, "c.json"))
            },
        ),
        "add: expected nested/c.json via production watcher (gen={last_gen}, len={})",
        playlist.len()
    );
    assert_playlist_invariants(&playlist);
    let truth = discover_animations(&root).unwrap();
    assert_eq!(
        playlist_path_set(&playlist),
        truth.into_iter().collect(),
        "add: playlist diverged from production discover_animations"
    );

    // --- Modify ---
    let b_path = playlist
        .entries()
        .iter()
        .find(|e| path_ends_with(&e.relative, "b.json"))
        .unwrap()
        .path
        .clone();
    playlist.select_filtered_index(
        playlist
            .entries()
            .iter()
            .position(|e| e.path == b_path)
            .unwrap(),
    );
    let selected_before = playlist.selected_path().map(PathBuf::from);
    let gen_before = last_gen;
    assert!(
        wait_until(
            &rx,
            &mut playlist,
            &mut last_gen,
            Duration::from_secs(15),
            || write_corrupt(&b_path),
            |p, generation| {
                generation > gen_before && p.entries().iter().any(|e| e.path == b_path)
            },
        ),
        "modify: expected generation advance while keeping b.json"
    );
    assert_eq!(
        playlist.selected_path().map(PathBuf::from),
        selected_before,
        "modify: selection should stay on the same path"
    );
    assert_playlist_invariants(&playlist);

    // --- Rename ---
    let a_path = playlist
        .entries()
        .iter()
        .find(|e| path_ends_with(&e.relative, "a.json"))
        .unwrap()
        .path
        .clone();
    let renamed = root.join("renamed.json");
    playlist.select_filtered_index(
        playlist
            .entries()
            .iter()
            .position(|e| e.path == a_path)
            .unwrap(),
    );
    assert!(
        wait_until(
            &rx,
            &mut playlist,
            &mut last_gen,
            Duration::from_secs(15),
            || {
                if a_path.exists() {
                    let _ = fs::rename(&a_path, &renamed);
                }
            },
            |p, _| {
                !p.entries().iter().any(|e| e.path == a_path)
                    && p.entries()
                        .iter()
                        .any(|e| path_ends_with(&e.relative, "renamed.json"))
            },
        ),
        "rename: old path gone and renamed.json present"
    );
    assert_playlist_invariants(&playlist);

    // --- Delete ---
    let delete_target = playlist
        .entries()
        .iter()
        .find(|e| path_ends_with(&e.relative, "b.json"))
        .map(|e| e.path.clone())
        .expect("b.json present");
    playlist.select_filtered_index(
        playlist
            .entries()
            .iter()
            .position(|e| e.path == delete_target)
            .unwrap(),
    );
    assert!(
        wait_until(
            &rx,
            &mut playlist,
            &mut last_gen,
            Duration::from_secs(15),
            || {
                let _ = fs::remove_file(&delete_target);
            },
            |p, _| !p.entries().iter().any(|e| e.path == delete_target),
        ),
        "delete: removed path must leave playlist"
    );
    assert!(playlist.selected_path().is_some() || playlist.is_empty());
    if let Some(sel) = playlist.selected_path() {
        assert_ne!(sel, delete_target.as_path());
    }
    assert_playlist_invariants(&playlist);

    let truth = discover_animations(&root).unwrap();
    assert_eq!(
        playlist_path_set(&playlist),
        truth.into_iter().collect(),
        "final playlist must match production discovery"
    );

    drop(session);
    drop(rx);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn watcher_generation_is_monotonic() {
    let tmp = temp_dir("watch-gen");
    write_valid(&tmp.join("a.json"));
    let (mut playlist, mut last_gen, rx, session, root) = boot_playlist(tmp);
    let mut seen = vec![last_gen];

    for i in 0..5 {
        let prev = last_gen;
        let path = root.join(format!("n{i}.json"));
        assert!(
            wait_until(
                &rx,
                &mut playlist,
                &mut last_gen,
                Duration::from_secs(15),
                || write_valid(&path),
                |_, generation| generation > prev,
            ),
            "expected generation > {prev}, got {last_gen}"
        );
        seen.push(last_gen);
    }

    for window in seen.windows(2) {
        assert!(
            window[1] >= window[0],
            "generations not monotonic: {seen:?}"
        );
    }

    drop(session);
    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn watcher_does_not_pick_up_symlink_loop_targets() {
    use std::os::unix::fs::symlink;

    let tmp = temp_dir("watch-symlink");
    write_valid(&tmp.join("real.json"));
    symlink(&tmp, tmp.join("loop")).unwrap();

    let (mut playlist, mut last_gen, rx, session, root) = boot_playlist(tmp);
    assert_eq!(playlist.len(), 1);

    let real = root.join("real.json");
    let _ = wait_until(
        &rx,
        &mut playlist,
        &mut last_gen,
        Duration::from_secs(5),
        || write_valid(&real),
        |p, _| p.len() == 1,
    );
    assert_eq!(playlist.len(), 1);
    assert_playlist_invariants(&playlist);

    drop(session);
    let _ = fs::remove_dir_all(&root);
}
