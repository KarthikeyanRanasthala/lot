//! Integration tests for production `Playlist` + `discover_animations`.
//!
//! These import `lot::playlist` only — no mirrored discovery or sort logic.

mod common;

use common::{assert_playlist_invariants, temp_dir, write_corrupt, write_valid};
use lot::input::LoadedInput;
use lot::playlist::{Playlist, discover_animations};
use std::fs;
use std::path::PathBuf;

#[test]
fn discover_animations_finds_json_and_lottie_recursively() {
    let root = temp_dir("discover-basic");
    write_valid(&root.join("top.json"));
    write_valid(&root.join("nested").join("deep.lottie"));
    fs::write(root.join("nested").join("skip.txt"), b"x").unwrap();
    write_valid(&root.join("nested").join("other.JSON"));

    let found = discover_animations(&root).unwrap();
    let names: Vec<_> = found
        .iter()
        .map(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase()
        })
        .collect();
    assert!(names.contains(&"top.json".into()));
    assert!(names.contains(&"deep.lottie".into()));
    assert!(names.contains(&"other.json".into()));
    assert!(!names.iter().any(|n| n == "skip.txt"));

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn discover_animations_natural_sorts() {
    let root = temp_dir("discover-natural");
    for name in ["animation10.json", "animation2.json", "animation1.json"] {
        write_valid(&root.join(name));
    }
    let found = discover_animations(&root).unwrap();
    let names: Vec<_> = found
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec!["animation1.json", "animation2.json", "animation10.json",]
    );
    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn discover_animations_does_not_follow_directory_symlinks() {
    use std::os::unix::fs::symlink;

    let root = temp_dir("discover-symlink");
    write_valid(&root.join("real.json"));
    symlink(&root, root.join("loop")).unwrap();

    let outside = temp_dir("discover-symlink-out");
    write_valid(&outside.join("outside.json"));
    symlink(&outside, root.join("link-out")).unwrap();

    let found = discover_animations(&root).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].file_name().unwrap(), "real.json");

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&outside);
}

#[test]
fn playlist_replace_preserves_and_rebinds_selection() {
    let root = PathBuf::from("/animations");
    let mut playlist = Playlist::new(root.clone());
    playlist.replace_entries(vec![
        root.join("a.json"),
        root.join("b.json"),
        root.join("c.json"),
    ]);
    playlist.select_filtered_index(1);
    assert_eq!(
        playlist.selected_path().unwrap().file_name().unwrap(),
        "b.json"
    );
    assert_playlist_invariants(&playlist);

    // Add unrelated file — selection preserved.
    playlist.replace_entries(vec![
        root.join("a.json"),
        root.join("b.json"),
        root.join("c.json"),
        root.join("d.json"),
    ]);
    assert_eq!(
        playlist.selected_path().unwrap().file_name().unwrap(),
        "b.json"
    );

    // Delete selected — nearby rebind.
    playlist.replace_entries(vec![root.join("a.json"), root.join("c.json")]);
    assert_eq!(
        playlist.selected_path().unwrap().file_name().unwrap(),
        "c.json"
    );
    assert_playlist_invariants(&playlist);
}

#[test]
fn playlist_search_is_filename_only() {
    let root = PathBuf::from("/animations");
    let mut playlist = Playlist::new(root.clone());
    playlist.replace_entries(vec![
        root.join("icons/loader.json"),
        root.join("other/icons.json"),
        root.join("icons/spinner.json"),
    ]);
    playlist.set_filter("icons");
    let visible: Vec<_> = playlist
        .visible_entries()
        .map(|(_, e)| e.display_name())
        .collect();
    assert_eq!(visible, vec!["other/icons.json"]);
    assert_playlist_invariants(&playlist);
}

#[test]
fn corrupt_file_does_not_block_selecting_sibling() {
    let root = temp_dir("corrupt-select");
    write_corrupt(&root.join("bad.json"));
    write_valid(&root.join("good.json"));

    let paths = discover_animations(&root).unwrap();
    let mut playlist = Playlist::new(root.canonicalize().unwrap());
    playlist.replace_entries(paths);
    assert_eq!(playlist.len(), 2);

    // Select corrupt entry and attempt load via production loader.
    let bad = playlist
        .entries()
        .iter()
        .find(|e| e.relative.ends_with("bad.json"))
        .unwrap()
        .path
        .clone();
    playlist.select_filtered_index(
        playlist
            .entries()
            .iter()
            .position(|e| e.path == bad)
            .unwrap(),
    );
    assert!(LoadedInput::from_path(&bad).is_err());

    // Navigate to sibling and load successfully.
    playlist.select_next(true);
    let selected = playlist.selected_path().unwrap().to_path_buf();
    if selected.ends_with("bad.json") {
        playlist.select_next(true);
    }
    let selected = playlist.selected_path().unwrap().to_path_buf();
    assert!(
        LoadedInput::from_path(&selected).is_ok(),
        "expected to load sibling after corrupt selection: {}",
        selected.display()
    );
    assert_playlist_invariants(&playlist);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn empty_directory_yields_empty_playlist() {
    let root = temp_dir("empty");
    fs::write(root.join("readme.txt"), b"hi").unwrap();
    let paths = discover_animations(&root).unwrap();
    assert!(paths.is_empty());
    let mut playlist = Playlist::new(root.canonicalize().unwrap());
    playlist.replace_entries(paths);
    assert!(playlist.is_empty());
    assert!(playlist.selected_path().is_none());
    let _ = fs::remove_dir_all(&root);
}
