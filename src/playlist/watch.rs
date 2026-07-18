use crate::playlist::discover::discover_animations;
use anyhow::{Context, Result};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Messages delivered from the background directory watcher to the UI thread.
#[derive(Debug)]
pub enum PlaylistEvent {
    /// Initial or subsequent full scan completed successfully.
    ///
    /// `generation` is monotonic so the UI can ignore stale scans that finish out of order.
    ScanComplete {
        generation: u64,
        paths: Vec<PathBuf>,
    },
    /// Discovery or watch setup failed. The playlist may still be usable with older data.
    Error { message: String },
}

/// RAII guard that stops the background watcher when dropped.
pub struct WatcherSession {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for WatcherSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            // The worker polls frequently; a short join is enough for clean tests.
            let _ = handle.join();
        }
    }
}

const DEBOUNCE: Duration = Duration::from_millis(200);
/// Back-pressure so a huge tree under constant churn cannot starve the worker with
/// back-to-back full rescans.
const MIN_SCAN_GAP: Duration = Duration::from_millis(500);
const IDLE_POLL: Duration = Duration::from_millis(50);

/// Spawn a background thread that performs an initial recursive scan, then watches
/// `root` for filesystem changes and re-scans after a short debounce.
///
/// The watcher never follows directory symlinks during discovery (see `discover_animations`).
/// FSEvents/inotify may report events under symlinked paths; each update still uses a
/// full rescan so the playlist stays consistent.
///
/// Drop the returned [`WatcherSession`] (or the process) to stop the thread.
pub fn spawn_directory_watcher(root: PathBuf) -> Result<(Receiver<PlaylistEvent>, WatcherSession)> {
    let root = root
        .canonicalize()
        .with_context(|| format!("could not resolve directory {}", root.display()))?;
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_worker = Arc::clone(&stop);
    let handle = thread::Builder::new()
        .name("lot-playlist-watch".into())
        .spawn(move || watch_loop(root, tx, stop_worker))
        .context("could not start playlist watcher thread")?;
    Ok((
        rx,
        WatcherSession {
            stop,
            handle: Some(handle),
        },
    ))
}

fn watch_loop(root: PathBuf, tx: Sender<PlaylistEvent>, stop: Arc<AtomicBool>) {
    let mut generation = 0_u64;
    let mut last_scan_at = Instant::now()
        .checked_sub(MIN_SCAN_GAP)
        .unwrap_or_else(Instant::now);

    if !emit_scan(&root, &tx, &mut generation, &mut last_scan_at) {
        return;
    }

    let (fs_tx, fs_rx) = mpsc::channel();
    let mut watcher = match RecommendedWatcher::new(
        move |result: notify::Result<Event>| {
            let _ = fs_tx.send(result);
        },
        notify::Config::default(),
    ) {
        Ok(watcher) => watcher,
        Err(error) => {
            let _ = tx.send(PlaylistEvent::Error {
                message: format!("could not start directory watcher: {error}"),
            });
            return;
        }
    };

    if let Err(error) = watcher.watch(&root, RecursiveMode::Recursive) {
        let _ = tx.send(PlaylistEvent::Error {
            message: format!("could not watch {}: {error}", root.display()),
        });
        // Without a watcher we still delivered the initial scan; exit cleanly.
        return;
    }

    // Keep the watcher alive for the duration of this thread.
    let _watcher = watcher;
    debounce_rescan_loop(
        &root,
        &fs_rx,
        &tx,
        &mut generation,
        &mut last_scan_at,
        &stop,
    );
}

fn emit_scan(
    root: &Path,
    tx: &Sender<PlaylistEvent>,
    generation: &mut u64,
    last_scan_at: &mut Instant,
) -> bool {
    *generation = generation.saturating_add(1);
    *last_scan_at = Instant::now();
    match discover_animations(root) {
        Ok(paths) => tx
            .send(PlaylistEvent::ScanComplete {
                generation: *generation,
                paths,
            })
            .is_ok(),
        Err(error) => tx
            .send(PlaylistEvent::Error {
                message: error.to_string(),
            })
            .is_ok(),
    }
}

fn debounce_rescan_loop(
    root: &Path,
    fs_rx: &Receiver<notify::Result<Event>>,
    tx: &Sender<PlaylistEvent>,
    generation: &mut u64,
    last_scan_at: &mut Instant,
    stop: &AtomicBool,
) {
    let mut pending_deadline: Option<Instant> = None;

    while !stop.load(Ordering::Relaxed) {
        let timeout = pending_deadline.map_or(IDLE_POLL, |deadline| {
            deadline
                .saturating_duration_since(Instant::now())
                .max(Duration::from_millis(1))
        });

        match fs_rx.recv_timeout(timeout) {
            Ok(Ok(_event)) => {
                // Coalesce bursts of events (save, atomic rename, recursive create).
                pending_deadline = Some(Instant::now() + DEBOUNCE);
            }
            Ok(Err(error)) => {
                if tx
                    .send(PlaylistEvent::Error {
                        message: format!("directory watch error: {error}"),
                    })
                    .is_err()
                {
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(deadline) = pending_deadline
                    && Instant::now() >= deadline
                {
                    let since_last = last_scan_at.elapsed();
                    if since_last < MIN_SCAN_GAP {
                        pending_deadline = Some(Instant::now() + (MIN_SCAN_GAP - since_last));
                        continue;
                    }
                    pending_deadline = None;
                    if !emit_scan(root, tx, generation, last_scan_at) {
                        return;
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("lot-watch-{label}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn watcher_emits_initial_scan_and_updates_on_create() {
        let root = temp_dir("create");
        fs::write(root.join("a.json"), b"{}").unwrap();

        let (rx, session) = spawn_directory_watcher(root.clone()).unwrap();

        let first = recv_scan(&rx, Duration::from_secs(5)).expect("initial scan");
        assert_eq!(first.len(), 1);

        // Create with a short retry loop: FSEvents can be slightly delayed, and the
        // worker also applies debounce + min-scan-gap back-pressure.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut saw_update = false;
        while Instant::now() < deadline {
            let _ = fs::write(root.join("b.json"), b"{}");
            if let Some(paths) = recv_scan_with_min_len(&rx, 2, Duration::from_millis(400)) {
                assert!(paths.len() >= 2);
                saw_update = true;
                break;
            }
        }
        assert!(
            saw_update,
            "expected playlist to include newly created file within 10s"
        );

        drop(session);
        drop(rx);
        let _ = fs::remove_dir_all(&root);
    }

    fn recv_scan(rx: &Receiver<PlaylistEvent>, timeout: Duration) -> Option<Vec<PathBuf>> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(PlaylistEvent::ScanComplete { paths, .. }) => return Some(paths),
                Ok(PlaylistEvent::Error { .. }) => continue,
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        None
    }

    fn recv_scan_with_min_len(
        rx: &Receiver<PlaylistEvent>,
        min_len: usize,
        timeout: Duration,
    ) -> Option<Vec<PathBuf>> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(PlaylistEvent::ScanComplete { paths, .. }) if paths.len() >= min_len => {
                    return Some(paths);
                }
                Ok(_) => continue,
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        None
    }
}
