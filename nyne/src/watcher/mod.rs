//! Filesystem watcher for source change propagation.
//!
//! Watches the source root for changes and propagates them to the
//! FUSE filesystem for provider notification and kernel cache invalidation.
//!
//! **Ignore-aware:** Uses the `ignore` crate to discover non-ignored
//! directories (respecting `.gitignore`). Only those directories receive
//! inotify watches, avoiding event storms from build output directories.
//!
//! **Debouncing:** Events are coalesced over a short window before being
//! forwarded to the chain providers.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr};
use notify::event::{CreateKind, ModifyKind};
use notify::{EventKind, RecursiveMode, Watcher};
use parking_lot::Mutex;
use tracing::{debug, trace, warn};

use crate::fuse::SharedNotifier;
use crate::fuse::inode_map::InodeMap;
use crate::fuse::notify::invalidate_inode_at;
use crate::router::Chain;

/// Debounce window: events are coalesced for this duration after the
/// last event arrives before being flushed.
const DEBOUNCE_TIMEOUT: Duration = Duration::from_millis(50);

/// Maximum delay from the first event in a batch to flush. Caps total
/// latency under sustained event rates (e.g., `cargo build`).
const MAX_BATCH_DELAY: Duration = Duration::from_millis(500);

/// Shared state from [`FuseFilesystem`](crate::fuse::FuseFilesystem) needed by the watcher.
pub struct WatcherBackend {
    pub chain: Arc<Chain>,
    pub inodes: Arc<InodeMap>,
    pub notifier: SharedNotifier,
}

/// Watches the real filesystem and propagates changes to the FUSE layer.
pub struct FsWatcher {
    _watcher: Arc<Mutex<notify::RecommendedWatcher>>,
    _event_thread: thread::JoinHandle<()>,
}

impl FsWatcher {
    /// Create a new filesystem watcher rooted at `watch_root`.
    pub fn new(watch_root: &Path, backend: WatcherBackend) -> Result<Self> {
        let (tx, rx) = mpsc::channel::<notify::Event>();

        let watcher = Arc::new(Mutex::new(
            notify::recommended_watcher({
                move |res: Result<notify::Event, notify::Error>| match res {
                    Ok(event) => {
                        let _ = tx.send(event);
                    }
                    Err(e) => warn!(error = %e, "filesystem watcher error"),
                }
            })
            .wrap_err("creating filesystem watcher")?,
        ));

        let dir_count = install_initial_watches(&watcher, watch_root);
        debug!(root = %watch_root.display(), directories = dir_count, "filesystem watcher started");

        let root = watch_root.to_path_buf();
        let watcher_ref = Arc::clone(&watcher);
        let event_thread = thread::Builder::new()
            .name("nyne-watcher".into())
            .spawn(move || event_loop(&rx, &root, &watcher_ref, &backend))
            .wrap_err("failed to spawn watcher event thread")?;

        Ok(Self {
            _watcher: watcher,
            _event_thread: event_thread,
        })
    }
}

fn install_initial_watches(watcher: &Mutex<notify::RecommendedWatcher>, root: &Path) -> usize {
    let mut w = watcher.lock();
    watch_directory_tree(&walk_builder(root), &mut w, false)
}

fn event_loop(
    rx: &Receiver<notify::Event>,
    watch_root: &Path,
    watcher: &Mutex<notify::RecommendedWatcher>,
    backend: &WatcherBackend,
) {
    let mut pending: HashSet<PathBuf> = HashSet::new();
    let mut batch_deadline: Option<Instant> = None;

    loop {
        if pending.is_empty() {
            let Ok(event) = rx.recv() else { break };
            process_raw_event(watch_root, watcher, &event, &mut pending);
            if !pending.is_empty() {
                batch_deadline = Some(Instant::now() + MAX_BATCH_DELAY);
            }
            continue;
        }

        let timeout = batch_deadline.map_or(DEBOUNCE_TIMEOUT, |d| {
            d.saturating_duration_since(Instant::now()).min(DEBOUNCE_TIMEOUT)
        });

        if !timeout.is_zero() {
            match rx.recv_timeout(timeout) {
                Ok(event) => {
                    process_raw_event(watch_root, watcher, &event, &mut pending);
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    flush(&mut pending, backend, &mut batch_deadline);
                    break;
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }

        flush(&mut pending, backend, &mut batch_deadline);
    }
}

fn process_raw_event(
    watch_root: &Path,
    watcher: &Mutex<notify::RecommendedWatcher>,
    event: &notify::Event,
    pending: &mut HashSet<PathBuf>,
) {
    if !is_relevant_event(event.kind) {
        return;
    }

    if matches!(event.kind, EventKind::Create(CreateKind::Folder)) {
        for path in &event.paths {
            watch_new_directory(watcher, path);
        }
    }

    for path in &event.paths {
        if let Some(rel) = to_relative(watch_root, path) {
            pending.insert(rel);
        }
    }
}

fn flush(pending: &mut HashSet<PathBuf>, backend: &WatcherBackend, batch_deadline: &mut Option<Instant>) {
    if pending.is_empty() {
        return;
    }
    let paths: Vec<PathBuf> = pending.drain().collect();
    trace!(count = paths.len(), "flushing debounced filesystem changes");

    // Notify chain providers → collect invalidation events.
    let events: Vec<_> = backend
        .chain
        .providers()
        .iter()
        .flat_map(|p| p.on_change(&paths))
        .collect();

    // Apply kernel notifications.
    if let Some(notifier) = backend.notifier.get() {
        for event in &events {
            invalidate_inode_at(&event.path, notifier.as_ref(), &backend.inodes);
        }
    }

    *batch_deadline = None;
}

fn watch_new_directory(watcher: &Mutex<notify::RecommendedWatcher>, path: &Path) {
    let mut w = watcher.lock();
    watch_directory_tree(&walk_builder(path), &mut w, true);
}

fn watch_directory_tree(
    builder: &ignore::WalkBuilder,
    watcher: &mut notify::RecommendedWatcher,
    skip_not_found: bool,
) -> usize {
    let mut count = 0;
    for entry in builder
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|ft| ft.is_dir()))
    {
        match watcher.watch(entry.path(), RecursiveMode::NonRecursive) {
            Ok(()) => count += 1,
            Err(e) if skip_not_found && matches!(e.kind, notify::ErrorKind::PathNotFound) => {}
            Err(e) => warn!(path = %entry.path().display(), error = %e, "failed to watch directory"),
        }
    }
    count
}

/// Build an ignore-aware directory walker.
fn walk_builder(walk_root: &Path) -> ignore::WalkBuilder {
    let mut builder = ignore::WalkBuilder::new(walk_root);
    builder.hidden(false);
    builder
}

/// Convert an absolute path under `watch_root` to a relative path.
fn to_relative(watch_root: &Path, path: &Path) -> Option<PathBuf> {
    let relative = path.strip_prefix(watch_root).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    relative.to_str()?; // skip non-UTF-8
    Some(relative.to_path_buf())
}

const fn is_relevant_event(kind: EventKind) -> bool {
    match kind {
        EventKind::Modify(ModifyKind::Metadata(_)) => false,
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_) => true,
        _ => false,
    }
}
