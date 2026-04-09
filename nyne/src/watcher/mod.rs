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

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};
use std::{mem, thread};

use color_eyre::eyre::{Result, WrapErr};
use notify::event::{CreateKind, ModifyKind};
use notify::{EventKind, RecursiveMode, Watcher};
use parking_lot::Mutex;
use tracing::{debug, trace, warn};

use crate::fuse::inode_map::InodeMap;
use crate::fuse::notify::propagate_source_changes;
use crate::fuse::{InlineWrites, SharedNotifier};
use crate::path_utils::PathExt;
use crate::router::Chain;

/// Debounce window: events are coalesced for this duration after the
/// last event arrives before being flushed.
const DEBOUNCE_TIMEOUT: Duration = Duration::from_millis(50);

/// Maximum delay from the first event in a batch to flush. Caps total
/// latency under sustained event rates (e.g., `cargo build`).
const MAX_BATCH_DELAY: Duration = Duration::from_millis(500);
/// How long after an inline write we suppress matching fsnotify echoes.
/// fsnotify latency is typically <5ms; the generous window guards against
/// loaded systems or lazy event delivery. Entries are evicted lazily on
/// watcher reads.
const INLINE_WRITE_SUPPRESSION_TTL: Duration = Duration::from_secs(2);

/// Shared state from [`FuseFilesystem`](crate::fuse::FuseFilesystem) needed by the watcher.
pub struct WatcherBackend {
    pub(crate) chain: Arc<Chain>,
    pub(crate) inodes: Arc<InodeMap>,
    pub(crate) notifier: SharedNotifier,
    pub(crate) inline_writes: InlineWrites,
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
            .spawn(move || {
                EventLoop {
                    rx,
                    watch_root: root,
                    watcher: watcher_ref,
                    backend,
                }
                .run();
            })
            .wrap_err("failed to spawn watcher event thread")?;

        Ok(Self {
            _watcher: watcher,
            _event_thread: event_thread,
        })
    }
}

struct EventLoop {
    rx: Receiver<notify::Event>,
    watch_root: PathBuf,
    watcher: Arc<Mutex<notify::RecommendedWatcher>>,
    backend: WatcherBackend,
}

impl EventLoop {
    fn run(&self) {
        let mut pending: HashSet<PathBuf> = HashSet::new();
        let mut batch_deadline: Option<Instant> = None;

        loop {
            if pending.is_empty() {
                let Ok(event) = self.rx.recv() else { break };
                self.process_raw_event(&event, &mut pending);
                if !pending.is_empty() {
                    batch_deadline = Some(Instant::now() + MAX_BATCH_DELAY);
                }
                continue;
            }

            let timeout = batch_deadline.map_or(DEBOUNCE_TIMEOUT, |d| {
                d.saturating_duration_since(Instant::now()).min(DEBOUNCE_TIMEOUT)
            });

            if !timeout.is_zero() {
                match self.rx.recv_timeout(timeout) {
                    Ok(event) => {
                        self.process_raw_event(&event, &mut pending);
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        self.flush(&mut pending);
                        break;
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                }
            }

            self.flush(&mut pending);
            batch_deadline = None;
        }
    }

    fn process_raw_event(&self, event: &notify::Event, pending: &mut HashSet<PathBuf>) {
        if !is_relevant_event(event.kind) {
            return;
        }

        if matches!(event.kind, EventKind::Create(CreateKind::Folder)) {
            for path in &event.paths {
                self.watch_new_directory(path);
            }
        }

        let now = Instant::now();
        let mut writes = self
            .backend
            .inline_writes
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        evict_expired_inline_writes(&mut writes, now);

        for path in &event.paths {
            let Some(rel) = path.strip_root(&self.watch_root) else {
                continue;
            };
            let rel = rel.to_path_buf();
            if writes.remove(&rel).is_some() {
                trace!(path = %rel.display(), "watcher: suppressed fsnotify echo of inline write");
                continue;
            }
            pending.insert(rel);
        }
    }

    fn flush(&self, pending: &mut HashSet<PathBuf>) {
        if pending.is_empty() {
            return;
        }
        let paths: Vec<PathBuf> = mem::take(pending).into_iter().collect();
        trace!(count = paths.len(), "flushing debounced filesystem changes");

        if let Some(notifier) = self.backend.notifier.get() {
            propagate_source_changes(&paths, &self.backend.chain, notifier.as_ref(), &self.backend.inodes);
        }
    }

    /// Walk the new directory tree outside the lock, then register watches.
    fn watch_new_directory(&self, path: &Path) {
        let dirs: Vec<PathBuf> = ignore::WalkBuilder::new(path)
            .hidden(false)
            .build()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_some_and(|ft| ft.is_dir()))
            .map(ignore::DirEntry::into_path)
            .collect();

        let mut w = self.watcher.lock();
        for dir in &dirs {
            match w.watch(dir, RecursiveMode::NonRecursive) {
                Ok(()) => {}
                Err(e) if matches!(e.kind, notify::ErrorKind::PathNotFound) => {}
                Err(e) => warn!(path = %dir.display(), error = %e, "failed to watch directory"),
            }
        }
    }
}

/// Register watches for all non-ignored directories under `root`.
///
/// Called once at startup before the event loop starts, so no lock contention.
fn install_initial_watches(watcher: &Mutex<notify::RecommendedWatcher>, root: &Path) -> usize {
    let mut w = watcher.lock();
    let mut count = 0;
    for entry in ignore::WalkBuilder::new(root)
        .hidden(false)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|ft| ft.is_dir()))
    {
        match w.watch(entry.path(), RecursiveMode::NonRecursive) {
            Ok(()) => count += 1,
            Err(e) => warn!(path = %entry.path().display(), error = %e, "failed to watch directory"),
        }
    }
    count
}

const fn is_relevant_event(kind: EventKind) -> bool {
    match kind {
        EventKind::Modify(ModifyKind::Metadata(_)) => false,
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_) => true,
        _ => false,
    }
}

/// Drop entries from the inline-write suppression map whose insertion
/// time is older than [`INLINE_WRITE_SUPPRESSION_TTL`]. Called lazily
/// from [`EventLoop::process_raw_event`] so the map stays bounded
/// without a background sweeper thread.
fn evict_expired_inline_writes(writes: &mut HashMap<PathBuf, Instant>, now: Instant) {
    writes.retain(|_, stamp| now.duration_since(*stamp) < INLINE_WRITE_SUPPRESSION_TTL);
}

#[cfg(test)]
mod tests;
