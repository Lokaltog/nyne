use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rstest::rstest;

use super::*;
use crate::router::GenerationMap;

fn test_generations() -> Arc<GenerationMap> { Arc::new(GenerationMap::new()) }

#[rstest]
fn hit_returns_cached_value() {
    let gens = test_generations();
    let cache = GenCache::new(gens);
    let source = PathBuf::from("foo.rs");
    let calls = AtomicUsize::new(0);

    let v1 = cache.get_or_compute("key", || {
        calls.fetch_add(1, Ordering::Relaxed);
        (42, source.clone())
    });
    let v2 = cache.get_or_compute("key", || {
        calls.fetch_add(1, Ordering::Relaxed);
        (99, source.clone())
    });

    assert_eq!(v1, 42);
    assert_eq!(v2, 42, "second call should return cached value");
    assert_eq!(calls.load(Ordering::Relaxed), 1, "compute called only once");
}

#[rstest]
fn stale_entry_recomputes() {
    let gens = test_generations();
    let cache = GenCache::new(gens.clone());
    let source = PathBuf::from("foo.rs");

    let v1 = cache.get_or_compute("key", || ("old", source.clone()));
    assert_eq!(v1, "old");

    // Bump generation -- cached entry is now stale.
    gens.bump(&source);

    let v2 = cache.get_or_compute("key", || ("new", source.clone()));
    assert_eq!(v2, "new", "stale entry should trigger recompute");
}

#[rstest]
fn invalidate_forces_recompute() {
    let gens = test_generations();
    let cache = GenCache::new(gens);
    let source = PathBuf::from("foo.rs");

    cache.get_or_compute("key", || ("first", source.clone()));
    cache.invalidate(&"key");

    let v = cache.get_or_compute("key", || ("second", source.clone()));
    assert_eq!(v, "second", "invalidated entry should recompute");
}
