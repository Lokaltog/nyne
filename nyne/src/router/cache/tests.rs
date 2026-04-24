use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rstest::rstest;

use super::*;
use crate::router::GenerationMap;

fn test_generations() -> Arc<GenerationMap> { Arc::new(GenerationMap::new()) }

/// Exercises `GenCache::get_or_compute` across three post-first-compute actions:
/// * no action — cached value is returned on second call (single compute).
/// * bump generation — entry becomes stale, second call recomputes.
/// * explicit invalidate — second call recomputes.
#[rstest]
#[case::hit_returns_cached(None, "first", 1)]
#[case::stale_recomputes(Some(Action::BumpGeneration), "second", 2)]
#[case::invalidate_recomputes(Some(Action::Invalidate), "second", 2)]
fn cache_behavior(#[case] action: Option<Action>, #[case] expected_v2: &str, #[case] expected_calls: usize) {
    let gens = test_generations();
    let cache = GenCache::new(gens.clone());
    let source = PathBuf::from("foo.rs");
    let calls = AtomicUsize::new(0);
    let compute = |v: &'static str| {
        calls.fetch_add(1, Ordering::Relaxed);
        (v, source.clone())
    };

    assert_eq!(cache.get_or_compute("key", || compute("first")), "first");

    match action {
        Some(Action::BumpGeneration) => {
            gens.bump(&source);
        }
        Some(Action::Invalidate) => cache.invalidate(&"key"),
        None => {}
    }

    assert_eq!(cache.get_or_compute("key", || compute("second")), expected_v2);
    assert_eq!(calls.load(Ordering::Relaxed), expected_calls);
}

enum Action {
    BumpGeneration,
    Invalidate,
}
