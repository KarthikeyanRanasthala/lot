use anyhow::{Context, Result};
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

/// Returns true when `path` has a supported animation extension (case-insensitive).
pub fn is_animation_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            let lower = ext.to_ascii_lowercase();
            lower == "json" || lower == "lottie"
        })
}

/// Recursively discover supported animation files under `root`.
///
/// Directory symlinks are not followed, which prevents recursion loops and keeps
/// discovery bounded to the real tree under `root`. File symlinks are included when
/// their link path has a supported extension.
pub fn discover_animations(root: &Path) -> Result<Vec<PathBuf>> {
    let root = root
        .canonicalize()
        .with_context(|| format!("could not resolve directory {}", root.display()))?;
    let mut found = Vec::new();
    visit_dir(&root, &mut found)?;
    found.sort_by(|a, b| natural_path_cmp(a, b));
    found.dedup();
    Ok(found)
}

fn visit_dir(dir: &Path, found: &mut Vec<PathBuf>) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            // Unreadable subdirectories are skipped so one bad folder cannot abort discovery.
            // The root is checked via canonicalize before the walk starts.
            let _ = error;
            return Ok(());
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };

        if file_type.is_symlink() {
            // Never recurse through directory symlinks. Include file symlinks by
            // extension only when the target is a regular file.
            if is_animation_path(&path) {
                match fs::metadata(&path) {
                    Ok(meta) if meta.is_file() => found.push(path),
                    _ => {}
                }
            }
            continue;
        }

        if file_type.is_dir() {
            visit_dir(&path, found)?;
        } else if file_type.is_file() && is_animation_path(&path) {
            found.push(path);
        }
    }

    Ok(())
}

/// Compare paths with natural (human) ordering on each component.
///
/// `animation2` sorts before `animation10`. Comparison is case-insensitive for
/// non-digit runs so ordering is stable across typical filenames.
pub fn natural_path_cmp(a: &Path, b: &Path) -> Ordering {
    let mut a_components = a.components();
    let mut b_components = b.components();

    loop {
        match (a_components.next(), b_components.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ac), Some(bc)) => {
                let a_os = ac.as_os_str();
                let b_os = bc.as_os_str();
                if a_os == b_os {
                    continue;
                }
                let a_str = a_os.to_string_lossy();
                let b_str = b_os.to_string_lossy();
                let order = natural_str_cmp(&a_str, &b_str);
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

pub fn natural_str_cmp(a: &str, b: &str) -> Ordering {
    let mut a_chars = a.chars().peekable();
    let mut b_chars = b.chars().peekable();

    loop {
        match (a_chars.peek().copied(), b_chars.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ac), Some(bc)) => {
                if ac.is_ascii_digit() && bc.is_ascii_digit() {
                    let a_num = take_number(&mut a_chars);
                    let b_num = take_number(&mut b_chars);
                    // Compare numeric values; on tie, fewer leading zeros / shorter digit
                    // run first is handled by comparing the raw digit strings after value.
                    match a_num.value.cmp(&b_num.value) {
                        Ordering::Equal => match a_num.digits.len().cmp(&b_num.digits.len()) {
                            Ordering::Equal => {}
                            non_eq => return non_eq,
                        },
                        non_eq => return non_eq,
                    }
                } else {
                    let a_chunk = take_non_digits(&mut a_chars);
                    let b_chunk = take_non_digits(&mut b_chars);
                    let order = a_chunk
                        .to_ascii_lowercase()
                        .cmp(&b_chunk.to_ascii_lowercase());
                    if order != Ordering::Equal {
                        return order;
                    }
                    // Preserve case-sensitive order when equal ignoring case.
                    match a_chunk.cmp(&b_chunk) {
                        Ordering::Equal => {}
                        non_eq => return non_eq,
                    }
                }
            }
        }
    }
}

struct NumberChunk {
    value: u128,
    digits: String,
}

fn take_number(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> NumberChunk {
    let mut digits = String::new();
    while let Some(c) = chars.peek().copied() {
        if c.is_ascii_digit() {
            digits.push(c);
            chars.next();
        } else {
            break;
        }
    }
    let value = digits.parse::<u128>().unwrap_or(u128::MAX);
    NumberChunk { value, digits }
}

fn take_non_digits(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut chunk = String::new();
    while let Some(c) = chars.peek().copied() {
        if c.is_ascii_digit() {
            break;
        }
        chunk.push(c);
        chars.next();
    }
    chunk
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("lot-discover-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"{}").unwrap();
    }

    #[test]
    fn natural_sort_puts_animation2_before_animation10() {
        let mut names = vec![
            "animation10.json",
            "animation2.json",
            "animation1.json",
            "animation20.json",
        ];
        names.sort_by(|a, b| natural_str_cmp(a, b));
        assert_eq!(
            names,
            vec![
                "animation1.json",
                "animation2.json",
                "animation10.json",
                "animation20.json",
            ]
        );
    }

    #[test]
    fn natural_path_cmp_orders_nested_paths() {
        let a = Path::new("/root/sub2/a.json");
        let b = Path::new("/root/sub10/a.json");
        assert_eq!(natural_path_cmp(a, b), Ordering::Less);
    }

    #[test]
    fn discovers_json_and_lottie_recursively() {
        let root = temp_dir("basic");
        touch(&root.join("top.json"));
        touch(&root.join("nested").join("deep.lottie"));
        touch(&root.join("nested").join("skip.txt"));
        touch(&root.join("nested").join("other.JSON"));

        let found = discover_animations(&root).unwrap();
        let names: Vec<_> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert!(names.iter().any(|n| n.eq_ignore_ascii_case("top.json")));
        assert!(names.iter().any(|n| n.eq_ignore_ascii_case("deep.lottie")));
        assert!(names.iter().any(|n| n.eq_ignore_ascii_case("other.JSON")));
        assert!(!names.iter().any(|n| n == "skip.txt"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn does_not_follow_directory_symlinks() {
        let root = temp_dir("symlink-loop");
        let real = root.join("real");
        fs::create_dir_all(&real).unwrap();
        touch(&real.join("a.json"));

        // Symlink that would recurse into root if followed.
        symlink(&root, root.join("loop")).unwrap();
        // Symlink to a sibling directory containing an animation.
        let outside = temp_dir("symlink-outside");
        touch(&outside.join("outside.json"));
        symlink(&outside, root.join("link-out")).unwrap();

        let found = discover_animations(&root).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file_name().unwrap(), "a.json");

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn includes_file_symlinks_with_animation_extension() {
        let root = temp_dir("file-symlink");
        let target = root.join("target.json");
        touch(&target);
        symlink(&target, root.join("alias.json")).unwrap();

        let found = discover_animations(&root).unwrap();
        assert_eq!(found.len(), 2);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn is_animation_path_is_case_insensitive() {
        assert!(is_animation_path(Path::new("x.JSON")));
        assert!(is_animation_path(Path::new("x.Lottie")));
        assert!(!is_animation_path(Path::new("x.txt")));
        assert!(!is_animation_path(Path::new("x")));
    }
}
