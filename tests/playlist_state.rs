//! Integration-style tests for playlist discovery and state transitions
//! (add / modify / rename / remove) using real temporary directories.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("lot-playlist-state-{label}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_valid(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(
        path,
        include_bytes!("../fixtures/two_frames.json").as_slice(),
    )
    .unwrap();
}

fn write_corrupt(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, b"{not valid json").unwrap();
}

// Integration tests cannot access private playlist modules, so this file mirrors
// discovery rules for filesystem add/modify/rename/remove ground truth. Full
// selection/filter state is covered by unit tests in `src/playlist/state.rs`.

fn discover_names(root: &Path) -> Vec<String> {
    // Mirror product discovery rules for integration assertions without private APIs.
    fn visit(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_symlink() {
                continue; // directory/file symlink policy: integration checks skip
            }
            if ft.is_dir() {
                visit(&path, out);
            } else if ft.is_file()
                && let Some(ext) = path.extension().and_then(|e| e.to_str())
            {
                let lower = ext.to_ascii_lowercase();
                if lower == "json" || lower == "lottie" {
                    out.push(path);
                }
            }
        }
    }

    let mut paths = Vec::new();
    visit(root, &mut paths);
    paths.sort_by(|a, b| {
        let a = a.strip_prefix(root).unwrap_or(a).to_string_lossy();
        let b = b.strip_prefix(root).unwrap_or(b).to_string_lossy();
        natural_cmp(&a, &b)
    });
    paths
        .into_iter()
        .map(|p| {
            p.strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    // Lightweight mirror for integration assertions (unit tests own the real impl).
    let mut ac = a.chars().peekable();
    let mut bc = b.chars().peekable();
    loop {
        match (ac.peek().copied(), bc.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(a_ch), Some(b_ch)) => {
                if a_ch.is_ascii_digit() && b_ch.is_ascii_digit() {
                    let mut an = String::new();
                    let mut bn = String::new();
                    while ac.peek().is_some_and(|c| c.is_ascii_digit()) {
                        an.push(ac.next().unwrap());
                    }
                    while bc.peek().is_some_and(|c| c.is_ascii_digit()) {
                        bn.push(bc.next().unwrap());
                    }
                    let av: u128 = an.parse().unwrap_or(u128::MAX);
                    let bv: u128 = bn.parse().unwrap_or(u128::MAX);
                    match av.cmp(&bv) {
                        std::cmp::Ordering::Equal => {}
                        other => return other,
                    }
                } else {
                    let al = a_ch.to_ascii_lowercase();
                    let bl = b_ch.to_ascii_lowercase();
                    match al.cmp(&bl) {
                        std::cmp::Ordering::Equal => {
                            ac.next();
                            bc.next();
                        }
                        other => return other,
                    }
                }
            }
        }
    }
}

#[test]
fn add_modify_rename_remove_update_ground_truth_listing() {
    let root = temp_dir("churn");
    write_valid(&root.join("animation1.json"));
    write_valid(&root.join("animation10.json"));
    write_valid(&root.join("animation2.json"));

    let initial = discover_names(&root);
    assert_eq!(
        initial,
        vec!["animation1.json", "animation2.json", "animation10.json",]
    );

    // Add
    write_valid(&root.join("nested").join("extra.json"));
    let after_add = discover_names(&root);
    assert!(after_add.iter().any(|n| n.ends_with("extra.json")));
    assert_eq!(after_add.len(), 4);

    // Modify (content change should keep the entry present)
    write_corrupt(&root.join("animation2.json"));
    let after_modify = discover_names(&root);
    assert_eq!(after_modify.len(), 4);
    assert!(after_modify.iter().any(|n| n == "animation2.json"));

    // Rename
    fs::rename(root.join("animation1.json"), root.join("renamed.json")).unwrap();
    let after_rename = discover_names(&root);
    assert!(!after_rename.iter().any(|n| n == "animation1.json"));
    assert!(after_rename.iter().any(|n| n == "renamed.json"));
    assert_eq!(after_rename.len(), 4);

    // Remove
    fs::remove_file(root.join("animation10.json")).unwrap();
    let after_remove = discover_names(&root);
    assert!(!after_remove.iter().any(|n| n == "animation10.json"));
    assert_eq!(after_remove.len(), 3);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn corrupt_file_does_not_break_headless_of_sibling() {
    let root = temp_dir("corrupt-sibling");
    write_corrupt(&root.join("bad.json"));
    let good = root.join("good.json");
    write_valid(&good);

    // Corrupt file is still "discovered" by extension.
    let names = discover_names(&root);
    assert_eq!(names.len(), 2);

    // Sibling remains loadable in headless mode.
    assert_cmd::Command::cargo_bin("lot")
        .unwrap()
        .args([
            good.to_str().unwrap(),
            "--headless",
            "--width",
            "4",
            "--height",
            "3",
            "--fps",
            "5",
        ])
        .assert()
        .success();

    // Corrupt file fails cleanly without hanging.
    assert_cmd::Command::cargo_bin("lot")
        .unwrap()
        .args([
            root.join("bad.json").to_str().unwrap(),
            "--headless",
            "--width",
            "4",
            "--height",
            "3",
            "--fps",
            "5",
        ])
        .assert()
        .failure();

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn empty_directory_has_no_animation_files() {
    let root = temp_dir("empty");
    fs::write(root.join("readme.txt"), b"hi").unwrap();
    assert!(discover_names(&root).is_empty());
    let _ = fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn directory_symlink_is_not_entered_for_listing_mirror() {
    use std::os::unix::fs::symlink;

    let root = temp_dir("symlink");
    write_valid(&root.join("real.json"));
    symlink(&root, root.join("loop")).unwrap();

    // Even a naive non-following walk sees only the real file.
    let names = discover_names(&root);
    assert_eq!(names, vec!["real.json".to_owned()]);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn natural_order_places_animation2_before_animation10() {
    let root = temp_dir("natural");
    for name in ["animation10.json", "animation2.json", "animation1.json"] {
        write_valid(&root.join(name));
    }
    assert_eq!(
        discover_names(&root),
        vec!["animation1.json", "animation2.json", "animation10.json",]
    );
    let _ = fs::remove_dir_all(&root);
}
