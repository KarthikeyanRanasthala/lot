//! Adversarial stress test for directory playlist discovery and state.
//!
//! Default `cargo test` ignores this test. Run it explicitly for ~5 minutes:
//!
//! ```sh
//! LOT_STRESS_SECS=300 cargo test --test playlist_stress -- --ignored --nocapture
//! ```
//!
//! Optional: `LOT_STRESS_SEED=42` for reproducibility.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const VALID_JSON: &[u8] = include_bytes!("../fixtures/two_frames.json");

#[derive(Clone, Debug)]
struct Entry {
    #[allow(dead_code)]
    path: PathBuf,
    corrupt: bool,
}

/// Pure playlist model used by the stress harness (mirrors production invariants).
struct StressPlaylist {
    root: PathBuf,
    entries: BTreeMap<PathBuf, Entry>,
    order: Vec<PathBuf>,
    filter: String,
    selected: Option<PathBuf>,
}

impl StressPlaylist {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            entries: BTreeMap::new(),
            order: Vec::new(),
            filter: String::new(),
            selected: None,
        }
    }

    fn apply_rescan(&mut self, paths: Vec<(PathBuf, bool)>) {
        let previous = self.selected.clone();
        self.entries.clear();
        for (path, corrupt) in paths {
            self.entries.insert(
                path.clone(),
                Entry {
                    path: path.clone(),
                    corrupt,
                },
            );
        }
        self.rebuild_order();
        self.rebind_selection(previous.as_deref());
    }

    fn set_filter(&mut self, filter: String) {
        let previous = self.selected.clone();
        self.filter = filter;
        self.rebind_selection(previous.as_deref());
    }

    fn select_next(&mut self) {
        let visible = self.visible();
        if visible.is_empty() {
            self.selected = None;
            return;
        }
        let pos = self
            .selected
            .as_ref()
            .and_then(|s| visible.iter().position(|p| p == s))
            .unwrap_or(0);
        self.selected = Some(visible[(pos + 1) % visible.len()].clone());
    }

    fn visible(&self) -> Vec<PathBuf> {
        let needle = self.filter.to_ascii_lowercase();
        self.order
            .iter()
            .filter(|path| {
                if needle.is_empty() {
                    return true;
                }
                path.strip_prefix(&self.root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .contains(&needle)
            })
            .cloned()
            .collect()
    }

    fn rebuild_order(&mut self) {
        let mut paths: Vec<PathBuf> = self.entries.keys().cloned().collect();
        paths.sort_by(|a, b| {
            let a = a
                .strip_prefix(&self.root)
                .unwrap_or(a)
                .to_string_lossy()
                .into_owned();
            let b = b
                .strip_prefix(&self.root)
                .unwrap_or(b)
                .to_string_lossy()
                .into_owned();
            natural_cmp(&a, &b)
        });
        self.order = paths;
    }

    fn rebind_selection(&mut self, previous: Option<&Path>) {
        let visible = self.visible();
        if let Some(prev) = previous {
            if visible.iter().any(|p| p == prev) {
                self.selected = Some(prev.to_path_buf());
                return;
            }
            if self.entries.contains_key(prev) {
                // Hidden by filter — pick first visible.
                self.selected = visible.into_iter().next();
                return;
            }
            // Removed: nearby by natural order.
            if visible.is_empty() {
                self.selected = None;
                return;
            }
            let pos = self
                .order
                .iter()
                .position(|p| p >= prev)
                .unwrap_or(self.order.len().saturating_sub(1));
            let candidate = self
                .order
                .get(pos)
                .cloned()
                .or_else(|| self.order.last().cloned());
            if let Some(c) = candidate
                && visible.iter().any(|p| p == &c)
            {
                self.selected = Some(c);
                return;
            }
            self.selected = visible.into_iter().next();
            return;
        }
        self.selected = visible.into_iter().next();
    }

    fn assert_invariants(&self) {
        // No duplicates in order.
        let unique: BTreeSet<_> = self.order.iter().collect();
        assert_eq!(unique.len(), self.order.len(), "duplicate playlist entries");

        // Order covers exactly the entry map.
        assert_eq!(self.order.len(), self.entries.len());
        for path in &self.order {
            assert!(
                self.entries.contains_key(path),
                "order path missing from map: {}",
                path.display()
            );
        }

        // Natural order holds.
        for pair in self.order.windows(2) {
            let a = pair[0]
                .strip_prefix(&self.root)
                .unwrap_or(&pair[0])
                .to_string_lossy();
            let b = pair[1]
                .strip_prefix(&self.root)
                .unwrap_or(&pair[1])
                .to_string_lossy();
            assert!(
                natural_cmp(&a, &b) != std::cmp::Ordering::Greater,
                "natural order violated: {a} > {b}"
            );
        }

        // Selection is valid or none.
        if let Some(selected) = &self.selected {
            assert!(
                self.entries.contains_key(selected),
                "selected path missing: {}",
                selected.display()
            );
        }

        // No ignored extensions.
        for path in &self.order {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            assert!(
                ext == "json" || ext == "lottie",
                "non-animation entry present: {}",
                path.display()
            );
        }
    }
}

fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
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
                    match av.cmp(&bv).then_with(|| an.len().cmp(&bn.len())) {
                        std::cmp::Ordering::Equal => {}
                        other => return other,
                    }
                } else {
                    match a_ch
                        .to_ascii_lowercase()
                        .cmp(&b_ch.to_ascii_lowercase())
                        .then_with(|| a_ch.cmp(&b_ch))
                    {
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

fn discover_disk(root: &Path) -> Vec<(PathBuf, bool)> {
    fn visit(dir: &Path, out: &mut Vec<(PathBuf, bool)>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_symlink() {
                // Match product: do not follow directory symlinks; include file symlinks
                // only when metadata says file.
                if let Ok(meta) = fs::metadata(&path)
                    && meta.is_file()
                {
                    push_if_animation(&path, out);
                }
                continue;
            }
            if ft.is_dir() {
                visit(&path, out);
            } else if ft.is_file() {
                push_if_animation(&path, out);
            }
        }
    }

    fn push_if_animation(path: &Path, out: &mut Vec<(PathBuf, bool)>) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext != "json" && ext != "lottie" {
            return;
        }
        let bytes = fs::read(path).unwrap_or_default();
        let corrupt = !is_objectish_json(&bytes);
        out.push((path.to_path_buf(), corrupt));
    }

    let mut found = Vec::new();
    visit(root, &mut found);
    found
}

/// Cheap corrupt-file heuristic for stress invariants (not a full Lottie validator).
fn is_objectish_json(bytes: &[u8]) -> bool {
    let trimmed = trim_ascii_ws(bytes);
    trimmed.first() == Some(&b'{') && trimmed.last() == Some(&b'}') && trimmed.len() > 2
}

fn trim_ascii_ws(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(0, |i| i + 1);
    if start >= end {
        &[]
    } else {
        &bytes[start..end]
    }
}

struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self {
            state: seed | 1, // avoid zero state
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn gen_range(&mut self, max: usize) -> usize {
        if max == 0 {
            return 0;
        }
        (self.next_u64() as usize) % max
    }
}

#[test]
#[ignore = "five-minute adversarial playlist stress; run with LOT_STRESS_SECS=300 cargo test --test playlist_stress -- --ignored --nocapture"]
fn adversarial_playlist_churn_for_five_minutes() {
    let secs: u64 = std::env::var("LOT_STRESS_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let seed: u64 = std::env::var("LOT_STRESS_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0x10_07);

    let root = std::env::temp_dir().join(format!(
        "lot-stress-{}-{}",
        seed,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();

    // Seed thousands of files (hardlink/copy template content).
    let seed_count = 2_500usize;
    let template = root.join("_template.json");
    fs::write(&template, VALID_JSON).unwrap();
    for i in 0..seed_count {
        let batch = root.join(format!("batch_{:04}", i / 100));
        fs::create_dir_all(&batch).unwrap();
        let dest = batch.join(format!("f{i}.json"));
        // Prefer hardlink to save disk; fall back to copy.
        if fs::hard_link(&template, &dest).is_err() {
            fs::copy(&template, &dest).unwrap();
        }
    }
    // Natural-sort stress names.
    for name in [
        "file1.json",
        "file2.json",
        "file10.json",
        "file20.json",
        "file100.json",
    ] {
        let dest = root.join(name);
        let _ = fs::hard_link(&template, &dest).or_else(|_| fs::copy(&template, &dest).map(|_| ()));
    }
    // Corrupt samples.
    for i in 0..50 {
        fs::write(root.join(format!("corrupt_{i}.json")), b"{bad").unwrap();
    }
    // Noise that must never enter the playlist.
    fs::write(root.join("readme.txt"), b"noise").unwrap();
    fs::write(root.join("image.png"), b"png").unwrap();

    let playlist = Arc::new(Mutex::new(StressPlaylist::new(root.clone())));
    {
        let mut pl = playlist.lock().unwrap();
        pl.apply_rescan(discover_disk(&root));
        pl.assert_invariants();
    }

    let stop = Arc::new(AtomicBool::new(false));
    let heartbeat = Arc::new(AtomicU64::new(0));
    let ops = Arc::new(AtomicU64::new(0));
    let max_batch_ms = Arc::new(AtomicU64::new(0));
    let max_rescan_ms = Arc::new(AtomicU64::new(0));
    let max_entries = Arc::new(AtomicU64::new(0));

    // Watchdog: fail if apply loop stalls for > 5s.
    let watchdog_stop = Arc::clone(&stop);
    let watchdog_beat = Arc::clone(&heartbeat);
    let watchdog = thread::spawn(move || {
        let mut last = 0_u64;
        let mut stalled_since = Instant::now();
        while !watchdog_stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(200));
            let beat = watchdog_beat.load(Ordering::Relaxed);
            if beat != last {
                last = beat;
                stalled_since = Instant::now();
            } else if stalled_since.elapsed() > Duration::from_secs(5) {
                panic!("playlist stress watchdog: apply loop unresponsive for >5s");
            }
        }
    });

    // User simulation: search + navigate.
    let user_stop = Arc::clone(&stop);
    let user_pl = Arc::clone(&playlist);
    let user_ops = Arc::clone(&ops);
    let user = thread::spawn(move || {
        let mut rng = XorShift64::new(seed ^ 0xabc);
        while !user_stop.load(Ordering::Relaxed) {
            {
                let mut pl = user_pl.lock().unwrap();
                match rng.gen_range(4) {
                    0 => pl.set_filter(String::new()),
                    1 => pl.set_filter("f1".into()),
                    2 => pl.set_filter("batch_00".into()),
                    _ => pl.set_filter("zzz_nope".into()),
                }
                pl.select_next();
                // Selecting a corrupt entry must not panic.
                if let Some(sel) = pl.selected.clone() {
                    let _ = pl.entries.get(&sel).map(|e| e.corrupt);
                }
                pl.assert_invariants();
            }
            user_ops.fetch_add(1, Ordering::Relaxed);
            thread::sleep(Duration::from_millis(20 + (rng.gen_range(30) as u64)));
        }
    });

    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut rng = XorShift64::new(seed);
    let mut next_id = seed_count as u64;
    let mut rescan_every = Instant::now() + Duration::from_secs(5);

    println!(
        "playlist stress starting: secs={secs} seed={seed} root={}",
        root.display()
    );

    while Instant::now() < deadline {
        let batch_start = Instant::now();
        let batch_ops = 40 + rng.gen_range(40);

        for _ in 0..batch_ops {
            let roll = rng.gen_range(100);
            if roll < 30 {
                // Create valid
                next_id += 1;
                let batch = root.join(format!("dyn_{:04}", next_id / 50));
                let _ = fs::create_dir_all(&batch);
                let dest = batch.join(format!("n{next_id}.json"));
                let _ = fs::hard_link(&template, &dest)
                    .or_else(|_| fs::copy(&template, &dest).map(|_| ()));
            } else if roll < 35 {
                // Create corrupt
                next_id += 1;
                let dest = root.join(format!("dyn_bad_{next_id}.json"));
                let _ = fs::write(&dest, b"{");
            } else if roll < 50 {
                // Modify random existing json
                if let Ok(paths) = collect_animation_paths(&root)
                    && !paths.is_empty()
                {
                    let path = &paths[rng.gen_range(paths.len())];
                    if rng.gen_range(2) == 0 {
                        let _ = fs::write(path, VALID_JSON);
                    } else {
                        let _ = fs::write(path, b"{partial");
                    }
                }
            } else if roll < 65 {
                // Rename
                if let Ok(paths) = collect_animation_paths(&root)
                    && !paths.is_empty()
                {
                    let from = paths[rng.gen_range(paths.len())].clone();
                    next_id += 1;
                    let to = root.join(format!("renamed_{next_id}.json"));
                    let _ = fs::rename(&from, &to);
                }
            } else if roll < 85 {
                // Delete
                if let Ok(paths) = collect_animation_paths(&root)
                    && paths.len() > 100
                {
                    let path = &paths[rng.gen_range(paths.len())];
                    let _ = fs::remove_file(path);
                }
            } else if roll < 90 {
                // Noise file
                next_id += 1;
                let _ = fs::write(root.join(format!("noise_{next_id}.tmp")), b"x");
            } else if roll < 95 {
                // Nested directory create
                next_id += 1;
                let dir = root.join("nested").join(format!("d{next_id}")).join("deep");
                let _ = fs::create_dir_all(&dir);
                let dest = dir.join(format!("deep_{next_id}.json"));
                let _ = fs::hard_link(&template, &dest)
                    .or_else(|_| fs::copy(&template, &dest).map(|_| ()));
            } else {
                // Symlink cycle attempt (unix)
                #[cfg(unix)]
                {
                    use std::os::unix::fs::symlink;
                    next_id += 1;
                    let loop_dir = root.join(format!("loop_{next_id}"));
                    let _ = fs::create_dir_all(&loop_dir);
                    let _ = symlink(&root, loop_dir.join("up"));
                }
            }
            ops.fetch_add(1, Ordering::Relaxed);
        }

        // Apply full rescan to playlist model (ground-truth journal mode).
        let rescan_start = Instant::now();
        let disk = discover_disk(&root);
        let rescan_ms = rescan_start.elapsed().as_millis() as u64;
        max_rescan_ms.fetch_max(rescan_ms, Ordering::Relaxed);
        assert!(
            rescan_ms < 10_000,
            "rescan too slow: {rescan_ms}ms (unresponsive scan)"
        );

        {
            let mut pl = playlist.lock().unwrap();
            pl.apply_rescan(disk);
            pl.assert_invariants();
            max_entries.fetch_max(pl.entries.len() as u64, Ordering::Relaxed);
        }

        let batch_ms = batch_start.elapsed().as_millis() as u64;
        max_batch_ms.fetch_max(batch_ms, Ordering::Relaxed);
        assert!(batch_ms < 15_000, "batch apply too slow: {batch_ms}ms");

        heartbeat.fetch_add(1, Ordering::Relaxed);

        if Instant::now() >= rescan_every {
            // Extra invariant: disk discovery matches playlist after apply.
            let disk = discover_disk(&root);
            let pl = playlist.lock().unwrap();
            assert_eq!(pl.entries.len(), disk.len(), "stale playlist vs disk");
            for (path, _) in &disk {
                assert!(
                    pl.entries.contains_key(path),
                    "disk path missing from playlist: {}",
                    path.display()
                );
            }
            rescan_every = Instant::now() + Duration::from_secs(5);
            println!(
                "stress progress: elapsed={:.0}s ops={} entries={} max_batch_ms={} max_rescan_ms={}",
                secs as f64
                    - deadline
                        .saturating_duration_since(Instant::now())
                        .as_secs_f64(),
                ops.load(Ordering::Relaxed),
                pl.entries.len(),
                max_batch_ms.load(Ordering::Relaxed),
                max_rescan_ms.load(Ordering::Relaxed),
            );
        }
    }

    stop.store(true, Ordering::Relaxed);
    let _ = user.join();
    let _ = watchdog.join();

    // Final reconciliation.
    let disk = discover_disk(&root);
    let pl = playlist.lock().unwrap();
    pl.assert_invariants();
    assert_eq!(pl.entries.len(), disk.len());

    println!(
        "playlist stress PASS: ops={} max_entries={} max_batch_ms={} max_rescan_ms={} final_entries={}",
        ops.load(Ordering::Relaxed),
        max_entries.load(Ordering::Relaxed),
        max_batch_ms.load(Ordering::Relaxed),
        max_rescan_ms.load(Ordering::Relaxed),
        pl.entries.len(),
    );

    drop(pl);
    let _ = fs::remove_dir_all(&root);
}

fn collect_animation_paths(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let ft = entry.file_type()?;
            if ft.is_dir() && !ft.is_symlink() {
                walk(&path, out)?;
            } else if ft.is_file()
                && let Some(ext) = path.extension().and_then(|e| e.to_str())
            {
                let lower = ext.to_ascii_lowercase();
                if lower == "json" || lower == "lottie" {
                    out.push(path);
                }
            }
        }
        Ok(())
    }
    walk(root, &mut out)?;
    Ok(out)
}
