use nyne::process::procfs::{COMM_MAX_LEN, read_ppid};
use rstest::rstest;

use super::*;

/// Build a `VisibilityMap` for tests, bundling the shared
/// [`ProcessNameCache`] that production wiring injects.
fn new_map(rules: impl IntoIterator<Item = (String, ProcessVisibility)>) -> VisibilityMap {
    VisibilityMap::new(rules, Arc::new(ProcessNameCache::default()))
}

/// Tests that resolve returns Default when no rules or overrides exist.
#[rstest]
fn resolve_returns_default_when_empty() {
    let map = new_map(std::iter::empty());
    // PID 1 (init) always exists on Linux.
    assert_eq!(map.resolve(1), ProcessVisibility::Default);
}

/// Tests that an explicit PID override takes precedence over other rules.
#[rstest]
fn pid_override_takes_precedence() {
    let map = new_map(std::iter::empty());
    map.set_pid(99999, ProcessVisibility::All);
    assert_eq!(map.resolve(99999), ProcessVisibility::All);
}

/// Tests that removing a PID override restores Default visibility.
#[rstest]
fn remove_pid_restores_default() {
    let map = new_map(std::iter::empty());
    map.set_pid(99999, ProcessVisibility::None);
    map.remove_pid(99999);
    assert_eq!(map.resolve(99999), ProcessVisibility::Default);
}

/// Tests that name rules are truncated to match kernel comm length.
#[rstest]
fn name_rule_truncation_matches_kernel() {
    // Names longer than 15 chars are truncated to match /proc/pid/comm.
    let map = new_map([("typescript-language-server".to_owned(), ProcessVisibility::None)]);
    // The stored key should be truncated to 15 chars.
    assert!(map.name_rules.contains_key("typescript-lang"));
}

/// Tests that a PID override shadows a matching name rule.
#[rstest]
fn pid_override_shadows_name_rule() {
    // Even if a name rule would match, a PID override wins.
    let map = new_map([("init".to_owned(), ProcessVisibility::None)]);
    map.set_pid(1, ProcessVisibility::All);
    assert_eq!(map.resolve(1), ProcessVisibility::All);
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

/// Tests that the ancestor walk stops at PID 1 (init).
#[rstest]
fn ancestor_walk_stops_at_init() {
    // PID 1 (init) has no override — should not inherit anything.
    let map = new_map(std::iter::empty());
    assert_eq!(map.resolve(1), ProcessVisibility::Default);
}
