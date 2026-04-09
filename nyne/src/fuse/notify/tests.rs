use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use color_eyre::eyre::Result;
use rstest::rstest;

use super::*;
use crate::fuse::inode_map::{InodeEntry, InodeMap, ROOT_INODE};
use crate::router::{Chain, InvalidationEvent, Next, Provider, ProviderId, ProviderMeta, Request};

/// Records every kernel notification so tests can assert on them.
#[derive(Default)]
struct RecordingNotifier {
    inval_inodes: Mutex<Vec<u64>>,
    inval_entries: Mutex<Vec<(u64, String)>>,
}

impl KernelNotifier for RecordingNotifier {
    fn inval_inode(&self, ino: u64) { self.inval_inodes.lock().unwrap().push(ino); }

    fn inval_entry(&self, parent_inode: u64, name: &str) {
        self.inval_entries.lock().unwrap().push((parent_inode, name.to_owned()));
    }
}

impl RecordingNotifier {
    fn invalidated_inodes(&self) -> Vec<u64> { self.inval_inodes.lock().unwrap().clone() }
}

/// Test provider: records `on_change` calls and returns pre-configured events.
struct RecordingProvider {
    id: ProviderId,
    seen: Mutex<Vec<PathBuf>>,
    events: Vec<InvalidationEvent>,
}

impl RecordingProvider {
    fn new(events: Vec<InvalidationEvent>) -> Self {
        Self {
            id: ProviderId::new("recorder"),
            seen: Mutex::new(Vec::new()),
            events,
        }
    }
}

impl ProviderMeta for RecordingProvider {
    fn id(&self) -> ProviderId { self.id }

    fn terminal(&self) -> bool { true }
}

impl Provider for RecordingProvider {
    fn accept(&self, _req: &mut Request, _next: &Next) -> Result<()> { Ok(()) }

    fn on_change(&self, changed: &[PathBuf]) -> Vec<InvalidationEvent> {
        self.seen.lock().unwrap().extend(changed.iter().cloned());
        self.events.clone()
    }
}

/// Builds an `InodeMap` pre-populated with every path in `paths`, plus
/// their unique parent directories. Returns the map alongside a lookup
/// table mapping each path to its allocated inode.
///
/// Deduplicates parent-dir allocation so a single directory entry maps
/// to a stable inode across all files under it — otherwise repeated
/// `allocate` calls for the same `(dir_path, name)` pair overwrite the
/// reverse index and break `invalidate_inode_at`'s parent lookup.
fn seeded_inodes<'a>(paths: impl IntoIterator<Item = &'a (&'a str, &'a str)>) -> (InodeMap, HashMap<PathBuf, u64>) {
    let inodes = InodeMap::new();
    let mut dir_inodes: HashMap<String, u64> = HashMap::new();
    let mut by_path: HashMap<PathBuf, u64> = HashMap::new();

    for &(dir, name) in paths {
        let parent_ino = *dir_inodes.entry(dir.to_owned()).or_insert_with(|| {
            inodes.allocate(InodeEntry {
                dir_path: PathBuf::new(),
                name: dir.to_owned(),
                parent_inode: ROOT_INODE,
            })
        });
        let file_ino = inodes.allocate(InodeEntry {
            dir_path: PathBuf::from(dir),
            name: name.to_owned(),
            parent_inode: parent_ino,
        });
        by_path.insert(PathBuf::from(dir).join(name), file_ino);
    }
    (inodes, by_path)
}

/// Build a chain with a single `RecordingProvider` that returns the given
/// derived events from `on_change`.
fn chain_with_events(events: Vec<InvalidationEvent>) -> (Chain, Arc<RecordingProvider>) {
    let provider = Arc::new(RecordingProvider::new(events));
    let chain = Chain::build(vec![provider.clone()]).unwrap();
    (chain, provider)
}

#[test]
fn propagate_empty_batch_is_noop() {
    let (chain, provider) = chain_with_events(vec![]);
    let inodes = InodeMap::new();
    let notifier = RecordingNotifier::default();

    propagate_source_changes(&[], &chain, &notifier, &inodes);

    assert!(
        provider.seen.lock().unwrap().is_empty(),
        "provider must not be called for empty batch"
    );
    assert!(notifier.invalidated_inodes().is_empty());
}

#[test]
fn propagate_skips_paths_with_no_allocated_inode() {
    // Paths never looked up have no allocated inode. The function must
    // silently skip them — the kernel has nothing cached there anyway.
    let (chain, _) = chain_with_events(vec![]);
    let inodes = InodeMap::new();
    let notifier = RecordingNotifier::default();

    propagate_source_changes(&[PathBuf::from("never/seen/before.rs")], &chain, &notifier, &inodes);

    assert!(notifier.invalidated_inodes().is_empty());
}

/// A batch of raw source paths plus zero or more derived provider events
/// must all land on `inval_inode`. The raw-path half of this assertion
/// is the regression guard: prior to `propagate_source_changes`,
/// `EventLoop::flush` only invalidated derived events, leaving the
/// kernel page cache serving stale `.git/index` content after external
/// `git commit`s outside the mount.
#[rstest]
#[case::raw_only(
    &[("src", "foo.rs")],
    &[],
)]
#[case::raw_plus_one_derived(
    &[(".git", "index")],
    &[(".git", "index@")],
)]
#[case::multiple_raw_and_multiple_derived(
    &[("src", "foo.rs"), ("src", "bar.rs")],
    &[("src", "foo.rs@"), ("src", "bar.rs@")],
)]
fn propagate_invalidates_raw_and_derived(#[case] raw: &[(&str, &str)], #[case] derived: &[(&str, &str)]) {
    // Pre-allocate every path we expect to check (raw + derived) so
    // `invalidate_inode_at` can find them via the reverse index.
    let (inodes, by_path) = seeded_inodes(raw.iter().chain(derived.iter()));
    let (chain, provider) = chain_with_events(
        derived
            .iter()
            .map(|(dir, name)| InvalidationEvent {
                path: PathBuf::from(dir).join(name),
            })
            .collect(),
    );
    let notifier = RecordingNotifier::default();
    let affected: Vec<PathBuf> = raw.iter().map(|(dir, name)| PathBuf::from(dir).join(name)).collect();

    propagate_source_changes(&affected, &chain, &notifier, &inodes);

    assert_eq!(
        *provider.seen.lock().unwrap(),
        affected,
        "provider saw the full affected batch"
    );
    for (path, ino) in &by_path {
        assert!(
            notifier.invalidated_inodes().contains(ino),
            "path {} (inode {ino}) must be invalidated (raw + derived must both land on inval_inode)",
            path.display(),
        );
    }
}
