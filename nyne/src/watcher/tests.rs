use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rstest::rstest;

use super::{INLINE_WRITE_SUPPRESSION_TTL, drain_pending_with_suppression, evict_expired_inline_writes};

/// Age of an entry relative to `now`, used to parametrize eviction cases.
#[derive(Clone, Copy)]
enum Age {
    /// `now - (TTL + margin)` — older than the window.
    Stale,
    /// `now - TTL` exactly — on the boundary, `duration_since == TTL` is NOT `< TTL`.
    Boundary,
    /// `now - margin` — well within the window.
    Fresh,
    /// `now` itself — just inserted.
    JustNow,
}

impl Age {
    fn stamp(self, now: Instant) -> Instant {
        match self {
            Self::Stale => now - INLINE_WRITE_SUPPRESSION_TTL - Duration::from_millis(1),
            Self::Boundary => now - INLINE_WRITE_SUPPRESSION_TTL,
            Self::Fresh => now - Duration::from_millis(50),
            Self::JustNow => now,
        }
    }
}

/// Expected outcome of running eviction over a single entry — avoids a
/// bare `bool` parameter on the parametrized test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Evicted,
    Retained,
}

#[rstest]
#[case::stale_is_evicted(Age::Stale, Outcome::Evicted)]
#[case::boundary_is_evicted(Age::Boundary, Outcome::Evicted)]
#[case::fresh_is_retained(Age::Fresh, Outcome::Retained)]
#[case::just_now_is_retained(Age::JustNow, Outcome::Retained)]
fn evict_expired_inline_writes_honours_ttl(#[case] age: Age, #[case] expected: Outcome) {
    let mut writes: HashMap<PathBuf, Instant> = HashMap::new();
    let now = Instant::now();
    let path = PathBuf::from("entry.rs");
    writes.insert(path.clone(), age.stamp(now));

    evict_expired_inline_writes(&mut writes, now);

    assert_eq!(
        if writes.contains_key(&path) {
            Outcome::Retained
        } else {
            Outcome::Evicted
        },
        expected,
    );
}

#[rstest]
#[case::empty_map(vec![])]
#[case::all_stale(vec![("a.rs", Age::Stale), ("b.rs", Age::Stale)])]
#[case::all_fresh(vec![("a.rs", Age::Fresh), ("b.rs", Age::JustNow)])]
#[case::mixed(vec![
    ("stale.rs", Age::Stale),
    ("boundary.rs", Age::Boundary),
    ("fresh.rs", Age::Fresh),
    ("right-now.rs", Age::JustNow),
])]
fn evict_expired_inline_writes_retains_only_fresh(#[case] entries: Vec<(&str, Age)>) {
    let mut writes: HashMap<PathBuf, Instant> = HashMap::new();
    let now = Instant::now();
    for (name, age) in &entries {
        writes.insert(PathBuf::from(*name), age.stamp(now));
    }

    evict_expired_inline_writes(&mut writes, now);

    let mut survivors: Vec<String> = writes.keys().map(|p| p.display().to_string()).collect();
    survivors.sort();
    let mut expected: Vec<String> = entries
        .into_iter()
        .filter(|(_, age)| matches!(age, Age::Fresh | Age::JustNow))
        .map(|(name, _)| name.to_owned())
        .collect();
    expected.sort();
    assert_eq!(survivors, expected);
}
/// Regression: fsnotify echo of an inline write must be suppressed even
/// when the echo lands in `pending` before the producer has stamped the
/// path into `inline_writes`.
///
/// The original implementation checked `inline_writes` at raw-event time
/// (inside `process_raw_event`), racing the FUSE write path — `fs::write`
/// emits the inotify event before `notify_change` has inserted into the
/// suppression map. A fast watcher thread would observe the event with
/// an empty map, forward it to `pending`, and then re-invalidate the
/// kernel page cache 50 ms later, corrupting rustc's incremental cache
/// during `cargo clippy --fix`.
///
/// Deferring the suppression check to flush time (where this helper is
/// called) closes the race because the producer has always finished
/// running `notify_change` by the time the debounce window elapses. This
/// test simulates the losing-the-race order: event is in `pending`
/// *before* the producer stamps the inline-write entry.
#[test]
fn drain_suppresses_echo_even_when_inline_writes_populated_after_event() {
    let path = PathBuf::from("src/foo.rs");
    let mut pending: HashSet<PathBuf> = HashSet::new();
    pending.insert(path.clone());

    // Producer lost the race: the event already sits in `pending`, and
    // only *now* does the FUSE write path finish populating the
    // suppression map. Flush-time check must still catch it.
    let mut writes: HashMap<PathBuf, Instant> = HashMap::new();
    writes.insert(path.clone(), Instant::now());

    assert!(
        drain_pending_with_suppression(&mut pending, &mut writes).is_empty(),
        "inline-write echo must be suppressed at flush time",
    );
    assert!(pending.is_empty(), "pending must be fully drained");
    assert!(
        !writes.contains_key(&path),
        "matched suppression entry must be consumed"
    );
}

/// External writes (never stamped into `inline_writes`) must be
/// propagated — suppression must never swallow genuine external changes.
#[test]
fn drain_forwards_external_writes_unchanged() {
    let external = PathBuf::from("src/external.rs");
    let mut pending: HashSet<PathBuf> = HashSet::new();
    pending.insert(external.clone());

    assert_eq!(drain_pending_with_suppression(&mut pending, &mut HashMap::new()), vec![
        external
    ],);
}

/// Each inline-write entry suppresses exactly one echo: if a second
/// fsnotify event for the same path arrives in the same batch (e.g. a
/// subsequent external write between the VFS write and flush), only the
/// first is suppressed. In practice `pending` is a `HashSet` so only one
/// event per path exists per batch — but a later batch containing the
/// same path must not be suppressed because the entry has been consumed.
#[test]
fn drain_consumes_suppression_entry_once() {
    let path = PathBuf::from("src/foo.rs");
    let mut writes: HashMap<PathBuf, Instant> = HashMap::new();
    writes.insert(path.clone(), Instant::now());

    // First batch: inline write echo — suppressed.
    let mut batch1: HashSet<PathBuf> = HashSet::new();
    batch1.insert(path.clone());
    assert!(drain_pending_with_suppression(&mut batch1, &mut writes).is_empty());
    assert!(!writes.contains_key(&path));

    // Second batch: a genuine later external write — must flow through.
    let mut batch2: HashSet<PathBuf> = HashSet::new();
    batch2.insert(path.clone());
    assert_eq!(drain_pending_with_suppression(&mut batch2, &mut writes), vec![path]);
}
