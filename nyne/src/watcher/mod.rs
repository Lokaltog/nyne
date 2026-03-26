//! Filesystem watcher for overlay change propagation.
//!
//! Watches the overlay merged view for changes and propagates them to the
//! router for cache invalidation and provider notification.
//!
//! **Ignore-aware watching:** Instead of watching the entire directory tree
//! recursively, the watcher uses the `ignore` crate to discover non-ignored
//! directories (respecting `.gitignore`, `.git/info/exclude`, and global
//! gitignore). Only those directories receive inotify watches. This avoids
//! installing thousands of watches on build output directories like `target/`
//! and prevents event storms during builds from churning the cache.
//!
//! **Dynamic watch management:** When a new directory is created inside a
//! watched directory, the watcher checks whether it (and its descendants)
//! should be watched. The `.git/` directory is always watched recursively
//! (it's bounded and needed for index/ref change detection).
//!
//! **Debouncing:** Events are coalesced over a short window before being
//! forwarded to the router. This prevents bursts of real-FS changes
//! (e.g., `git checkout`, `cargo build`) from hammering the router with
//! hundreds of individual invalidations that contend with in-flight FUSE
//! lookups on the L1 cache locks.
//!
//! **Overlay paths:** inotify on overlayfs merged views has known limitations
//! (events from lowerdir changes may not propagate). This is consistent with
//! the overlay design — lowerdir is read-only and changes there don't affect
//! the user's view.

use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::eyre::WrapErr;
use notify::event::{CreateKind, ModifyKind};
use notify::{EventKind, RecursiveMode, Watcher};
use parking_lot::Mutex;
use tracing::{debug, trace, warn};

use crate::dispatch::Router;
use crate::dispatch::path_filter::ignore_walk_builder;
use crate::prelude::*;

/// Debounce window: events are coalesced for this duration after the
/// last event arrives before being flushed to the router.
const DEBOUNCE_TIMEOUT: Duration = Duration::from_millis(50);

/// Maximum delay from the first event in a batch to flush. Caps total
/// latency under sustained event rates (e.g., `cargo build` writing
/// thousands of files). Without this, the debounce window resets on
/// every new event and the batch never flushes until the burst stops.
const MAX_BATCH_DELAY: Duration = Duration::from_millis(500);

/// Watches the real filesystem and propagates changes to the router.
///
/// Owns the underlying `notify::RecommendedWatcher` and an event
/// processing thread. When dropped, the watcher and sender are dropped,
/// which causes the event thread to drain remaining events and exit.
pub struct FsWatcher {
    _watcher: Arc<Mutex<notify::RecommendedWatcher>>,
    _event_thread: thread::JoinHandle<()>,
}

/// Construction and lifecycle for the filesystem watcher.
impl FsWatcher {
    /// Create a new filesystem watcher rooted at `watch_root`.
    ///
    /// `git_dir_name` is the first path component of the git metadata
    /// directory (derived from `git2::Repository::path()`). When `None`,
    /// defaults to `".git"`.
    pub fn new(watch_root: &Path, router: Arc<Router>, git_dir_name: Option<&str>) -> Result<Self> {
        let git_dir: Arc<str> = git_dir_name.unwrap_or(".git").into();
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

        let dir_count = install_initial_watches(&watcher, watch_root, &git_dir);
        debug!(
            root = %watch_root.display(),
            directories = dir_count,
            "filesystem watcher started (ignore-aware)"
        );

        let root = watch_root.to_path_buf();
        let watcher_ref = Arc::clone(&watcher);
        let git_dir_owned = Arc::clone(&git_dir);
        let event_thread = thread::Builder::new()
            .name("nyne-watcher".into())
            .spawn(move || event_loop(&rx, &root, &watcher_ref, &router, &git_dir_owned))
            .wrap_err("failed to spawn watcher event thread")?;

        Ok(Self {
            _watcher: watcher,
            _event_thread: event_thread,
        })
    }
}

/// Installs filesystem watches on all non-ignored directories under the root.
fn install_initial_watches(watcher: &Mutex<notify::RecommendedWatcher>, root: &Path, git_dir: &str) -> usize {
    let start = Instant::now();
    let mut w = watcher.lock();
    let mut count = 0;

    // Walk non-ignored directories. WalkBuilder handles all gitignore
    // filtering (project, local exclude, global). The shared builder
    // factory skips the git dir (handled separately below).
    let builder = ignore_walk_builder(root, root, git_dir);
    count += watch_directory_tree(&builder, &mut w, false);

    // The git directory is explicitly skipped by the walk builder
    // (WalkBuilder does NOT skip .git when hidden(false) is set, contrary
    // to its docs). Watch it separately with recursive mode — it's
    // bounded (no build artifacts) and we need events for index, refs,
    // and HEAD changes.
    let git_dir_path = root.join(git_dir);
    if git_dir_path.is_dir() {
        if let Err(e) = w.watch(&git_dir_path, RecursiveMode::Recursive) {
            warn!(error = %e, git_dir, "failed to watch git directory");
        } else {
            count += 1;
        }
    }

    debug!(
        directories = count,
        elapsed_ms = start.elapsed().as_millis(),
        "initial watch installation complete"
    );

    count
}

/// Main event loop: receives raw `notify::Event`s, manages watches for
/// new directories, converts paths to `VfsPath`s, debounces, and flushes
/// batches to the router.
fn event_loop(
    rx: &Receiver<notify::Event>,
    watch_root: &Path,
    watcher: &Mutex<notify::RecommendedWatcher>,
    router: &Router,
    git_dir: &str,
) {
    let mut pending: HashSet<VfsPath> = HashSet::new();
    let mut batch_deadline: Option<Instant> = None;

    loop {
        if pending.is_empty() {
            // Nothing accumulated — block until the next event (or sender drops).
            let Ok(event) = rx.recv() else { break };
            process_raw_event(watch_root, watcher, &event, &mut pending, git_dir);
            if !pending.is_empty() {
                batch_deadline = Some(Instant::now() + MAX_BATCH_DELAY);
            }
            continue;
        }

        // Events are pending — wait for more or flush on timeout.
        let timeout = batch_deadline.map_or(DEBOUNCE_TIMEOUT, |d| {
            d.saturating_duration_since(Instant::now()).min(DEBOUNCE_TIMEOUT)
        });

        if !timeout.is_zero() {
            match rx.recv_timeout(timeout) {
                Ok(event) => {
                    process_raw_event(watch_root, watcher, &event, &mut pending, git_dir);
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    flush(&mut pending, router, &mut batch_deadline);
                    break;
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }

        flush(&mut pending, router, &mut batch_deadline);
    }
}

/// Process a single raw `notify::Event`: install watches for new
/// directories, then convert paths to `VfsPath`s for debouncing.
fn process_raw_event(
    watch_root: &Path,
    watcher: &Mutex<notify::RecommendedWatcher>,
    event: &notify::Event,
    pending: &mut HashSet<VfsPath>,
    git_dir: &str,
) {
    if !is_relevant_event(event.kind) {
        return;
    }

    // New directory created — install watches if not gitignored.
    if matches!(event.kind, EventKind::Create(CreateKind::Folder)) {
        for path in &event.paths {
            watch_new_directory(watch_root, watcher, path, git_dir);
        }
    }

    for path in &event.paths {
        if let Some(vpath) = to_vfs_path(watch_root, path) {
            pending.insert(vpath);
        }
    }
}

/// Adds watches for a newly created directory and its non-ignored children.
fn watch_new_directory(watch_root: &Path, watcher: &Mutex<notify::RecommendedWatcher>, path: &Path, git_dir: &str) {
    // Skip directories inside the git dir — already covered by recursive watch.
    if let Ok(relative) = path.strip_prefix(watch_root)
        && relative.starts_with(git_dir)
    {
        return;
    }

    let builder = ignore_walk_builder(path, watch_root, git_dir);
    let mut w = watcher.lock();
    watch_directory_tree(&builder, &mut w, true);
}

/// Walk directories from a pre-configured `WalkBuilder` and install
/// non-recursive inotify watches on each discovered directory.
///
/// When `skip_not_found` is true, `PathNotFound` errors are silently
/// ignored — the directory may have been removed between walk and watch
/// installation (expected during dynamic watch management).
///
/// Returns the number of directories successfully watched.
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
        trace!(path = %entry.path().display(), "installing watch");
        match watcher.watch(entry.path(), RecursiveMode::NonRecursive) {
            Ok(()) => count += 1,
            Err(e) if skip_not_found && matches!(e.kind, notify::ErrorKind::PathNotFound) => {}
            Err(e) => {
                warn!(path = %entry.path().display(), error = %e, "failed to watch directory");
            }
        }
    }
    count
}

/// Drains pending change events and dispatches them to the router.
fn flush(pending: &mut HashSet<VfsPath>, router: &Router, batch_deadline: &mut Option<Instant>) {
    if pending.is_empty() {
        return;
    }

    let paths: Vec<VfsPath> = pending.drain().collect();

    trace!(
        count = paths.len(),
        paths = ?paths,
        "flushing debounced filesystem changes"
    );

    router.handle_fs_changes(&paths);
    *batch_deadline = None;
}

/// Convert an absolute path under `watch_root` to a `VfsPath`.
///
/// Returns `None` if the path is the watch root itself or cannot be
/// stripped of the prefix (e.g., a path outside the watch tree).
fn to_vfs_path(watch_root: &Path, path: &Path) -> Option<VfsPath> {
    let relative = path.strip_prefix(watch_root).ok()?;
    if relative.as_os_str().is_empty() {
        // Skip the watch root itself — a metadata change on the source
        // directory (e.g., chmod) should not nuke the entire cache.
        return None;
    }
    let Some(s) = relative.to_str() else {
        trace!(path = %path.display(), "skipping non-UTF-8 path");
        return None;
    };
    VfsPath::new(s).ok()
}

/// Returns whether a filesystem event kind affects content or structure.
const fn is_relevant_event(kind: EventKind) -> bool {
    match kind {
        // Metadata-only changes (chmod, chown, atime) don't affect
        // file content or directory structure — skip them.
        EventKind::Modify(ModifyKind::Metadata(_)) => false,
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_) => true,
        _ => false,
    }
}
