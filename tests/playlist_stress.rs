//! Five-minute adversarial stress test against **production** playlist + watcher.
//!
//! Uses only:
//! - [`lot::playlist::Playlist`]
//! - [`lot::playlist::discover_animations`]
//! - [`lot::playlist::spawn_directory_watcher`]
//! - [`lot::playlist::PlaylistEvent`] / generation values
//! - [`lot::input::LoadedInput`] for corrupt-file isolation
//!
//! Playlist updates come **only** from watcher `ScanComplete` events (same path as the TUI).
//! Ground-truth reconciliation uses production `discover_animations` after quiet periods.
//!
//! ```sh
//! LOT_STRESS_SECS=300 cargo test --test playlist_stress -- --ignored --nocapture
//! ```

mod common;

use common::{
    apply_playlist_event, assert_playlist_invariants, fixture_json_bytes, playlist_path_set,
    temp_dir, write_corrupt,
};
use lot::input::LoadedInput;
use lot::playlist::{Playlist, discover_animations, spawn_directory_watcher};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self { state: seed | 1 }
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
            0
        } else {
            (self.next_u64() as usize) % max
        }
    }
}

fn collect_animation_paths(root: &Path) -> Vec<PathBuf> {
    discover_animations(root).unwrap_or_default()
}

#[test]
#[ignore = "five-minute production watcher stress; LOT_STRESS_SECS=300 cargo test --test playlist_stress -- --ignored --nocapture"]
fn adversarial_playlist_churn_for_five_minutes() {
    let secs: u64 = std::env::var("LOT_STRESS_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let seed: u64 = std::env::var("LOT_STRESS_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0x10_07);

    let root = temp_dir(&format!("stress-{seed}"));
    let template = root.join("_template.json");
    fs::write(&template, fixture_json_bytes()).unwrap();

    let seed_count = 2_500usize;
    for i in 0..seed_count {
        let batch = root.join(format!("batch_{:04}", i / 100));
        fs::create_dir_all(&batch).unwrap();
        let dest = batch.join(format!("f{i}.json"));
        if fs::hard_link(&template, &dest).is_err() {
            fs::copy(&template, &dest).unwrap();
        }
    }
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
    for i in 0..50 {
        write_corrupt(&root.join(format!("corrupt_{i}.json")));
    }
    fs::write(root.join("readme.txt"), b"noise").unwrap();
    fs::write(root.join("image.png"), b"png").unwrap();

    let root = root.canonicalize().unwrap();
    let (rx, session) = spawn_directory_watcher(root.clone()).expect("spawn watcher");
    let mut playlist = Playlist::new(root.clone());
    let mut last_generation = 0_u64;

    // Initial scan must arrive from the production watcher (not a manual discover inject).
    let boot_deadline = Instant::now() + Duration::from_secs(30);
    let mut booted = false;
    while Instant::now() < boot_deadline {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(200))
            && apply_playlist_event(&mut playlist, event, &mut last_generation)
        {
            booted = true;
            break;
        }
    }
    assert!(
        booted,
        "production watcher did not emit initial ScanComplete"
    );
    assert!(playlist.len() >= seed_count, "initial playlist too small");
    assert_playlist_invariants(&playlist);

    let stop = Arc::new(AtomicBool::new(false));
    let pause_churn = Arc::new(AtomicBool::new(false));
    let heartbeat = Arc::new(AtomicU64::new(0));
    let ops = Arc::new(AtomicU64::new(0));
    let max_apply_ms = Arc::new(AtomicU64::new(0));
    let events_applied = Arc::new(AtomicU64::new(0));

    // Watchdog: fail if event processing stalls for >8s while test is running.
    let wd_stop = Arc::clone(&stop);
    let wd_beat = Arc::clone(&heartbeat);
    let watchdog = thread::spawn(move || {
        let mut last = 0_u64;
        let mut stalled_since = Instant::now();
        while !wd_stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(200));
            let beat = wd_beat.load(Ordering::Relaxed);
            if beat != last {
                last = beat;
                stalled_since = Instant::now();
            } else if stalled_since.elapsed() > Duration::from_secs(8) {
                panic!("stress watchdog: event loop unresponsive for >8s");
            }
        }
    });

    // Churn worker: real filesystem operations only.
    let churn_stop = Arc::clone(&stop);
    let churn_pause = Arc::clone(&pause_churn);
    let churn_root = root.clone();
    let churn_ops = Arc::clone(&ops);
    let template_path = template.clone();
    let churn = thread::spawn(move || {
        let mut rng = XorShift64::new(seed ^ 0xC0FFEE);
        let mut next_id = seed_count as u64;
        while !churn_stop.load(Ordering::Relaxed) {
            if churn_pause.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            let batch = 30 + rng.gen_range(40);
            for _ in 0..batch {
                let roll = rng.gen_range(100);
                if roll < 28 {
                    next_id += 1;
                    let batch_dir = churn_root.join(format!("dyn_{:04}", next_id / 50));
                    let _ = fs::create_dir_all(&batch_dir);
                    let dest = batch_dir.join(format!("n{next_id}.json"));
                    let _ = fs::hard_link(&template_path, &dest)
                        .or_else(|_| fs::copy(&template_path, &dest).map(|_| ()));
                } else if roll < 33 {
                    next_id += 1;
                    write_corrupt(&churn_root.join(format!("dyn_bad_{next_id}.json")));
                } else if roll < 50 {
                    let paths = collect_animation_paths(&churn_root);
                    if !paths.is_empty() {
                        let path = &paths[rng.gen_range(paths.len())];
                        if rng.gen_range(2) == 0 {
                            let _ = fs::write(path, fixture_json_bytes());
                        } else {
                            let _ = fs::write(path, b"{partial");
                        }
                    }
                } else if roll < 65 {
                    let paths = collect_animation_paths(&churn_root);
                    if !paths.is_empty() {
                        let from = paths[rng.gen_range(paths.len())].clone();
                        next_id += 1;
                        let to = churn_root.join(format!("renamed_{next_id}.json"));
                        let _ = fs::rename(&from, &to);
                    }
                } else if roll < 82 {
                    let paths = collect_animation_paths(&churn_root);
                    if paths.len() > 200 {
                        let path = &paths[rng.gen_range(paths.len())];
                        let _ = fs::remove_file(path);
                    }
                } else if roll < 88 {
                    next_id += 1;
                    let _ = fs::write(churn_root.join(format!("noise_{next_id}.tmp")), b"x");
                } else if roll < 94 {
                    next_id += 1;
                    let dir = churn_root
                        .join("nested")
                        .join(format!("d{next_id}"))
                        .join("deep");
                    let _ = fs::create_dir_all(&dir);
                    let dest = dir.join(format!("deep_{next_id}.json"));
                    let _ = fs::hard_link(&template_path, &dest)
                        .or_else(|_| fs::copy(&template_path, &dest).map(|_| ()));
                } else {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::symlink;
                        next_id += 1;
                        let loop_dir = churn_root.join(format!("loop_{next_id}"));
                        let _ = fs::create_dir_all(&loop_dir);
                        let _ = symlink(&churn_root, loop_dir.join("up"));
                    }
                }
                churn_ops.fetch_add(1, Ordering::Relaxed);
            }
            thread::sleep(Duration::from_millis(5));
        }
    });

    // Shared playlist for optional user thread — use mutex for concurrent select/filter.
    let shared = Arc::new(Mutex::new(Playlist::new(root.clone())));
    {
        let mut g = shared.lock().unwrap();
        *g = playlist.clone();
    }

    let user_stop = Arc::clone(&stop);
    let user_pl = Arc::clone(&shared);
    let user = thread::spawn(move || {
        let mut rng = XorShift64::new(seed ^ 0xA11);
        while !user_stop.load(Ordering::Relaxed) {
            {
                let mut pl = user_pl.lock().unwrap();
                match rng.gen_range(5) {
                    0 => pl.set_filter(String::new()),
                    1 => pl.set_filter("f1"),
                    2 => pl.set_filter("batch_00"),
                    3 => pl.set_filter("zzz_nope"),
                    _ => pl.set_filter("corrupt"),
                }
                pl.select_next(true);
                if let Some(path) = pl.selected_path().map(Path::to_path_buf) {
                    // Corrupt files must not panic the loader.
                    let _ = LoadedInput::from_path(&path);
                }
                assert_playlist_invariants(&pl);
            }
            thread::sleep(Duration::from_millis(25 + rng.gen_range(40) as u64));
        }
    });

    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut last_reconcile = Instant::now();

    println!(
        "playlist stress (PRODUCTION watcher) starting: secs={secs} seed={seed} root={}",
        root.display()
    );

    while Instant::now() < deadline {
        let apply_start = Instant::now();

        // Drain watcher events — this is the only path that mutates the authoritative playlist.
        loop {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(event) => {
                    if apply_playlist_event(&mut playlist, event, &mut last_generation) {
                        events_applied.fetch_add(1, Ordering::Relaxed);
                        assert_playlist_invariants(&playlist);
                        if let Ok(mut g) = shared.lock() {
                            *g = playlist.clone();
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    panic!("watcher channel disconnected unexpectedly");
                }
            }
            if apply_start.elapsed() > Duration::from_millis(500) {
                break;
            }
        }

        let apply_ms = apply_start.elapsed().as_millis() as u64;
        max_apply_ms.fetch_max(apply_ms, Ordering::Relaxed);
        assert!(
            apply_ms < 5_000,
            "event processing batch too slow: {apply_ms}ms"
        );
        heartbeat.fetch_add(1, Ordering::Relaxed);

        // Main-thread user interactions on the authoritative playlist as well.
        match (ops.load(Ordering::Relaxed) as usize) % 7 {
            0 => playlist.set_filter(String::new()),
            1 => playlist.set_filter("f1"),
            2 => playlist.set_filter("deep_"),
            3 => {
                playlist.set_filter(String::new());
                playlist.select_next(true);
            }
            4 => playlist.select_previous(true),
            5 => {
                if let Some(path) = playlist.selected_path().map(Path::to_path_buf) {
                    let _ = LoadedInput::from_path(&path);
                }
            }
            _ => {}
        }
        assert_playlist_invariants(&playlist);

        // Periodic reconciliation against production discovery after a forced rescan.
        if last_reconcile.elapsed() > Duration::from_secs(5) {
            pause_churn.store(true, Ordering::Relaxed);
            thread::sleep(Duration::from_millis(100)); // let in-flight churn ops finish

            let gen_before = last_generation;
            // Nudge the watched tree so notify emits at least one event after the pause.
            let tick = root.join(".lot-stress-tick");
            let _ = fs::write(
                &tick,
                format!("{}", Instant::now().elapsed().as_nanos()).as_bytes(),
            );

            let settle_deadline = Instant::now() + Duration::from_secs(4);
            while Instant::now() < settle_deadline {
                if let Ok(event) = rx.recv_timeout(Duration::from_millis(50))
                    && apply_playlist_event(&mut playlist, event, &mut last_generation)
                {
                    events_applied.fetch_add(1, Ordering::Relaxed);
                }
                if last_generation > gen_before {
                    // One more quiet half-second to catch trailing debounced scans.
                    let quiet = Instant::now() + Duration::from_millis(700);
                    while Instant::now() < quiet {
                        if let Ok(event) = rx.recv_timeout(Duration::from_millis(50))
                            && apply_playlist_event(&mut playlist, event, &mut last_generation)
                        {
                            events_applied.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    break;
                }
            }

            while let Ok(event) = rx.try_recv() {
                if apply_playlist_event(&mut playlist, event, &mut last_generation) {
                    events_applied.fetch_add(1, Ordering::Relaxed);
                }
            }

            let filter = playlist.filter().to_owned();
            playlist.set_filter(String::new());
            let truth = discover_animations(&root).expect("discover_animations");
            let truth_set: std::collections::BTreeSet<_> = truth.into_iter().collect();
            let pl_set = playlist_path_set(&playlist);
            if pl_set != truth_set {
                // One recovery pass: force another tick and rescan window.
                let gen2 = last_generation;
                let _ = fs::write(&tick, b"retry");
                let retry_deadline = Instant::now() + Duration::from_secs(3);
                while Instant::now() < retry_deadline {
                    if let Ok(event) = rx.recv_timeout(Duration::from_millis(50))
                        && apply_playlist_event(&mut playlist, event, &mut last_generation)
                    {
                        events_applied.fetch_add(1, Ordering::Relaxed);
                    }
                    if last_generation > gen2 {
                        break;
                    }
                }
                while let Ok(event) = rx.try_recv() {
                    if apply_playlist_event(&mut playlist, event, &mut last_generation) {
                        events_applied.fetch_add(1, Ordering::Relaxed);
                    }
                }
                playlist.set_filter(String::new());
                let truth = discover_animations(&root).expect("discover_animations");
                let truth_set: std::collections::BTreeSet<_> = truth.into_iter().collect();
                let pl_set = playlist_path_set(&playlist);
                let only_pl: Vec<_> = pl_set.difference(&truth_set).take(5).collect();
                let only_disk: Vec<_> = truth_set.difference(&pl_set).take(5).collect();
                assert_eq!(
                    pl_set, truth_set,
                    "playlist diverged from production discover_animations; only_in_playlist≈{only_pl:?} only_on_disk≈{only_disk:?}"
                );
            }
            for path in playlist.entries() {
                assert!(
                    path.path.exists(),
                    "stale playlist entry: {}",
                    path.path.display()
                );
            }
            playlist.set_filter(filter);
            assert_playlist_invariants(&playlist);
            last_reconcile = Instant::now();
            pause_churn.store(false, Ordering::Relaxed);
            println!(
                "stress progress: elapsed={:.0}s ops={} events={} entries={} gen={} max_apply_ms={}",
                secs as f64
                    - deadline
                        .saturating_duration_since(Instant::now())
                        .as_secs_f64(),
                ops.load(Ordering::Relaxed),
                events_applied.load(Ordering::Relaxed),
                playlist.len(),
                last_generation,
                max_apply_ms.load(Ordering::Relaxed),
            );
        }
    }

    stop.store(true, Ordering::Relaxed);
    let _ = churn.join();
    let _ = user.join();
    let _ = watchdog.join();

    // Final quiet settle: drain remaining watcher events.
    let settle = Instant::now() + Duration::from_secs(3);
    while Instant::now() < settle {
        if let Ok(event) = rx.recv_timeout(Duration::from_millis(100)) {
            let _ = apply_playlist_event(&mut playlist, event, &mut last_generation);
        }
    }
    playlist.set_filter(String::new());
    let truth = discover_animations(&root).unwrap();
    assert_eq!(
        playlist_path_set(&playlist),
        truth.into_iter().collect(),
        "final reconcile failed"
    );
    assert_playlist_invariants(&playlist);
    assert!(
        events_applied.load(Ordering::Relaxed) > 0,
        "no watcher events applied — stress did not exercise production watcher"
    );
    assert!(
        last_generation >= 1,
        "generation never advanced from production watcher"
    );

    // WatcherSession Drop joins the worker thread.
    let stop_start = Instant::now();
    drop(session);
    drop(rx);
    assert!(
        stop_start.elapsed() < Duration::from_secs(5),
        "watcher session did not stop promptly"
    );

    println!(
        "playlist stress PASS (production): ops={} events={} max_entries={} gen={} max_apply_ms={} final_entries={}",
        ops.load(Ordering::Relaxed),
        events_applied.load(Ordering::Relaxed),
        playlist.len(),
        last_generation,
        max_apply_ms.load(Ordering::Relaxed),
        playlist.len(),
    );

    let _ = fs::remove_dir_all(&root);
}
