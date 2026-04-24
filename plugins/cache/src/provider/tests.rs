use std::sync::Arc;
use std::sync::atomic::Ordering;

use nyne::router::{CachePolicy, NamedNode, Readable};
use nyne::test_support::{StubReadable, test_read_ctx};
use rstest::rstest;

use super::cached::{CachedReadable, wrap_readable};

#[test]
fn cached_readable_returns_correct_size_after_read() {
    let (stub, calls) = StubReadable::from_bytes(b"hello world").with_counter();
    let cached = CachedReadable {
        inner: Arc::new(stub),
        persistent: std::sync::OnceLock::new(),
        timed: std::sync::Mutex::new(None),
        // None → persistent mode (matches `CachePolicy::Default` outcome in `wrap_readable`).
        ttl: None,
    };

    // Before any read, size is unknown.
    assert_eq!(cached.size(), None);

    // First read populates cache.
    assert_eq!(cached.read(&test_read_ctx()).unwrap(), b"hello world");
    assert_eq!(cached.size(), Some(11));
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    // Second read returns cached content without calling inner.
    assert_eq!(cached.read(&test_read_ctx()).unwrap(), b"hello world");
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn cached_readable_delegates_backing_path() {
    assert_eq!(
        CachedReadable {
            inner: Arc::new(StubReadable::empty()),
            persistent: std::sync::OnceLock::new(),
            timed: std::sync::Mutex::new(None),
            ttl: None,
        }
        .backing_path(),
        None
    );
}

#[test]
fn wrap_readable_skips_nodes_with_backing_path() {
    // A node without readable should be unchanged.
    let mut node = NamedNode::file("test");
    wrap_readable(&mut node);
    assert!(node.readable().is_none());
}

#[test]
fn wrap_readable_wraps_virtual_readable() {
    let mut node = NamedNode::new(
        "test",
        nyne::router::Node::file().with_readable(StubReadable::from_bytes(b"content")),
    );

    // Before wrapping, size is None (StubReadable doesn't impl size).
    assert_eq!(node.readable().unwrap().size(), None);

    wrap_readable(&mut node);

    // Still None before read.
    assert_eq!(node.readable().unwrap().size(), None);

    // After read, size is correct.
    assert_eq!(node.readable().unwrap().read(&test_read_ctx()).unwrap(), b"content");
    assert_eq!(node.readable().unwrap().size(), Some(7));
}

#[test]
fn negative_lookup_cache_hit_does_not_restore_state() {
    use std::path::PathBuf;

    use nyne::router::{GenerationMap, Next, Op, Provider, Request};

    use super::CacheProvider;

    // A typed state marker to detect leakage.
    #[derive(Clone)]
    struct Marker;

    let provider = CacheProvider::new(Arc::new(GenerationMap::default()));
    let next = Next::empty();

    // First lookup: cache miss → closure runs next (empty chain → no results).
    // The closure captures req state (empty) alongside the negative result.
    let mut req = Request::new(PathBuf::new(), Op::Lookup { name: "missing".into() });
    provider.accept(&mut req, &next).unwrap();
    assert!(req.nodes.is_empty(), "nothing should resolve");

    // Second lookup: same key → cache hit with negative result.
    // Set Marker on a fresh request to verify it survives (is NOT replaced
    // by restore_state from the cached negative entry).
    let mut req2 = Request::new(PathBuf::new(), Op::Lookup { name: "missing".into() });
    req2.set_state(Marker);
    provider.accept(&mut req2, &next).unwrap();
    assert!(req2.nodes.is_empty());
    // Key assertion: Marker must still be present — negative cache hits
    // must not overwrite request state. This prevents the slice middleware's
    // speculative lookup from leaking companion state into subsequent lookups.
    assert!(
        req2.state::<Marker>().is_some(),
        "negative cache hit must not overwrite request state"
    );
}
#[test]
fn on_change_bumps_parent_generation() {
    use std::path::PathBuf;

    use nyne::router::{GenerationMap, Provider};

    use super::CacheProvider;

    // The cache provider stores `Lookup { name }` entries with
    // `source = req.path()` — the *parent* directory. When an external
    // change modifies a file, the watcher calls `on_change` with the file
    // path. Bumping only the file's own generation would leave the
    // lookup cache entry (keyed on the parent) still fresh. On_change
    // must bump the parent as well — this mirrors the mutation branch
    // of `accept`, which also bumps the parent via `source_from_request`.
    let generations = Arc::new(GenerationMap::default());
    let provider = CacheProvider::new(Arc::clone(&generations));

    let parent = std::path::Path::new(".git");
    let child = PathBuf::from(".git/index");

    // Baseline: unknown paths return generation 0.
    assert_eq!(generations.get(parent), 0);
    assert_eq!(generations.get(&child), 0);

    // Simulate a filesystem-watcher event for an externally modified file.
    let _ = provider.on_change(&[child.clone()]);

    // The child's own generation must bump (used by `source =
    // req.path()` cases where the child itself is a directory being
    // operated on).
    assert!(generations.get(&child) > 0, "child generation not bumped");
    // The parent's generation must also bump — this is the invariant
    // that fixes stale `Lookup` cache entries for external file
    // modifications. Without it, `git status` reading `.git/index`
    // inside the mount would keep returning a cached pre-commit
    // lookup after an external `git commit`.
    assert!(
        generations.get(parent) > 0,
        "parent generation not bumped — lookup cache entries keyed on the \
         parent would remain stale after external modifications to children"
    );
}

#[rstest]
#[case::no_cache(CachePolicy::NoCache)]
#[case::ttl_zero(CachePolicy::Ttl(std::time::Duration::ZERO))]
fn wrap_readable_skips_opt_out_policies(#[case] policy: CachePolicy) {
    // Both `NoCache` and `Ttl(Duration::ZERO)` are explicit opt-out signals:
    // the readable must NOT be wrapped, so consecutive reads see the live
    // inner state. Regression for the bug where a frozen "No changes." was
    // returned from staged.diff after the first read.
    let (stub, calls) = StubReadable::from_bytes(b"v1").with_counter();
    let mut node = NamedNode::new(
        "test",
        nyne::router::Node::file().with_readable(stub).with_cache_policy(policy),
    );

    wrap_readable(&mut node);

    for _ in 0..3 {
        node.readable()
            .expect("readable present after wrap_readable")
            .read(&test_read_ctx())
            .expect("inner readable read must succeed");
    }

    assert_eq!(
        calls.load(Ordering::Relaxed),
        3,
        "opt-out CachePolicy ({policy:?}) must not wrap — every read must hit the inner readable",
    );
}
