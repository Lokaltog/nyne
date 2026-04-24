use nyne::process::procfs::{COMM_MAX_LEN, read_ppid};
use rstest::rstest;

use super::*;

/// Build a `VisibilityMap` for tests, bundling the shared
/// [`ProcessNameCache`] that production wiring injects.
fn new_map(rules: impl IntoIterator<Item = (String, ProcessVisibility)>) -> VisibilityMap {
    VisibilityMap::new(rules, Arc::new(ProcessNameCache::default()))
}

/// Tests `resolve` across the precedence chain: empty map → Default,
/// explicit PID override wins, removing the override restores Default,
/// PID override shadows a matching name rule.
#[rstest]
#[case::empty_map_returns_default(vec![], &[], 1, ProcessVisibility::Default)]
#[case::explicit_pid_override(
    vec![],
    &[Op::SetPid(99999, ProcessVisibility::All)],
    99999, ProcessVisibility::All,
)]
#[case::remove_pid_restores_default(
    vec![],
    &[Op::SetPid(99999, ProcessVisibility::None), Op::RemovePid(99999)],
    99999, ProcessVisibility::Default,
)]
#[case::pid_shadows_name_rule(
    vec![("init", ProcessVisibility::None)],
    &[Op::SetPid(1, ProcessVisibility::All)],
    1, ProcessVisibility::All,
)]
fn resolve_scenarios(
    #[case] name_rules: Vec<(&str, ProcessVisibility)>,
    #[case] ops: &[Op],
    #[case] pid: u32,
    #[case] expected: ProcessVisibility,
) {
    let map = new_map(name_rules.into_iter().map(|(k, v)| (k.to_owned(), v)));
    for op in ops {
        match *op {
            Op::SetPid(p, v) => map.set_pid(p, v),
            Op::RemovePid(p) => {
                map.remove_pid(p);
            }
        }
    }
    assert_eq!(map.resolve(pid), expected);
}

#[derive(Clone, Copy)]
enum Op {
    SetPid(u32, ProcessVisibility),
    RemovePid(u32),
}

/// Tests that name rules are truncated to match kernel comm length.
#[rstest]
fn name_rule_truncation_matches_kernel() {
    // Names longer than 15 chars are truncated to match /proc/pid/comm.
    let map = new_map([("typescript-language-server".to_owned(), ProcessVisibility::None)]);
    // The stored key should be truncated to 15 chars.
    assert!(map.name_rules.contains_key("typescript-lang"));
}

/// Tests that dynamic name rules take precedence over static ones.
#[rstest]
fn dynamic_name_rule_takes_precedence_over_static() {
    let map = new_map([("test-proc".to_owned(), ProcessVisibility::None)]);
    map.set_name_rule("test-proc", ProcessVisibility::All);

    let rules = map.dynamic_name_rules();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0], ("test-proc".to_owned(), ProcessVisibility::All));
}

/// Tests that dynamic name rules truncate long names to `COMM_MAX_LEN`.
#[rstest]
fn dynamic_name_rule_truncates_long_names() {
    let map = new_map([]);
    let long_name = "a".repeat(20);
    map.set_name_rule(&long_name, ProcessVisibility::None);

    let rules = map.dynamic_name_rules();
    assert_eq!(rules[0].0.len(), COMM_MAX_LEN);
}

/// Tests that `explicit_pid_entries` excludes cached resolution entries.
#[rstest]
fn explicit_pid_entries_excludes_cached() {
    let map = new_map([]);
    map.set_pid(42, ProcessVisibility::All);
    // resolve() on a non-existent PID caches Default — should not appear in explicit entries.
    map.resolve(999);

    let entries = map.explicit_pid_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0], (42, ProcessVisibility::All));
}

/// Tests that a child process inherits its parent's explicit visibility.
#[rstest]
fn child_inherits_parent_visibility() {
    // Our own PID's parent (the test runner) should be walkable.
    let our_pid = std::process::id();
    let parent_pid = read_ppid(our_pid).expect("should be able to read our PPid");

    let map = new_map(std::iter::empty());
    map.set_pid(parent_pid, ProcessVisibility::All);

    // Our PID has no direct override, but should inherit from parent.
    assert_eq!(map.resolve(our_pid), ProcessVisibility::All);

    // The result should now be cached as Cached (not Explicit).
    assert_eq!(
        map.pid_entries.read().get(&our_pid).copied(),
        Some(VisibilityEntry::Cached(ProcessVisibility::All))
    );
}

/// Tests that cached entries do not propagate to child processes.
#[rstest]
fn cached_entry_does_not_propagate_to_children() {
    // A cached Default on a parent must NOT be inherited by children.
    // This was the root cause of git seeing VFS files when spawned by
    // a Default-visibility parent (e.g., Claude Code).
    let our_pid = std::process::id();
    let parent_pid = read_ppid(our_pid).expect("should be able to read our PPid");

    let map = new_map(std::iter::empty());

    // Simulate a cached Default on our parent (as if the parent made a
    // FUSE request and fell through to the Default fallback).
    map.pid_entries
        .write()
        .insert(parent_pid, VisibilityEntry::Cached(ProcessVisibility::Default));

    // Our PID should NOT inherit the cached Default — the ancestor walk
    // must skip Cached entries. We should get Default from the fallback
    // path, not from inheritance.
    assert_eq!(map.resolve(our_pid), ProcessVisibility::Default);

    // Verify our result is cached as Cached, not Explicit.
    assert_eq!(
        map.pid_entries.read().get(&our_pid).copied(),
        Some(VisibilityEntry::Cached(ProcessVisibility::Default))
    );
}

/// Tests that name-rule resolution goes through the shared
/// [`ProcessNameCache`], not a direct procfs read.
#[rstest]
fn name_rule_resolves_through_shared_cache() {
    // Pre-populate the cache with a known comm for our own PID.
    let our_pid = std::process::id();
    let cache = Arc::new(ProcessNameCache::default());
    let our_comm = cache.get_or_read(our_pid).expect("should read own comm via cache");

    // Name rule matches our comm — resolve must route through the shared cache.
    let map = VisibilityMap::new([(our_comm, ProcessVisibility::None)], Arc::clone(&cache));
    assert_eq!(map.resolve(our_pid), ProcessVisibility::None);
}
