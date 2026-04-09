use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rstest::rstest;

use super::{INLINE_WRITE_SUPPRESSION_TTL, evict_expired_inline_writes};

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
