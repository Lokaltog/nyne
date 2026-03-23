use super::*;

#[test]
fn resolve_returns_default_when_empty() {
    let map = VisibilityMap::new(std::iter::empty());
    // PID 1 (init) always exists on Linux.
    assert_eq!(map.resolve(1), ProcessVisibility::Default);
}

#[test]
fn pid_override_takes_precedence() {
    let map = VisibilityMap::new(std::iter::empty());
    map.set_pid(99999, ProcessVisibility::All);
    assert_eq!(map.resolve(99999), ProcessVisibility::All);
}

#[test]
fn remove_pid_restores_default() {
    let map = VisibilityMap::new(std::iter::empty());
    map.set_pid(99999, ProcessVisibility::None);
    map.remove_pid(99999);
    assert_eq!(map.resolve(99999), ProcessVisibility::Default);
}

#[test]
fn name_rule_truncation_matches_kernel() {
    // Names longer than 15 chars are truncated to match /proc/pid/comm.
    let map = VisibilityMap::new([("typescript-language-server".to_owned(), ProcessVisibility::None)]);
    // The stored key should be truncated to 15 chars.
    assert!(map.name_rules.contains_key("typescript-lang"));
}

#[test]
fn pid_override_shadows_name_rule() {
    // Even if a name rule would match, a PID override wins.
    let map = VisibilityMap::new([("init".to_owned(), ProcessVisibility::None)]);
    map.set_pid(1, ProcessVisibility::All);
    assert_eq!(map.resolve(1), ProcessVisibility::All);
}

#[test]
fn dynamic_name_rule_takes_precedence_over_static() {
    let map = VisibilityMap::new([("test-proc".to_owned(), ProcessVisibility::None)]);
    map.set_name_rule("test-proc".to_owned(), ProcessVisibility::All);

    let rules = map.dynamic_name_rules();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0], ("test-proc".to_owned(), ProcessVisibility::All));
}

#[test]
fn dynamic_name_rule_truncates_long_names() {
    let map = VisibilityMap::new([]);
    let long_name = "a".repeat(20);
    map.set_name_rule(long_name, ProcessVisibility::None);

    let rules = map.dynamic_name_rules();
    assert_eq!(rules[0].0.len(), COMM_MAX_LEN);
}

#[test]
fn explicit_pid_entries_excludes_cached() {
    let map = VisibilityMap::new([]);
    map.set_pid(42, ProcessVisibility::All);
    // resolve() on a non-existent PID caches Default — should not appear in explicit entries.
    map.resolve(999);

    let entries = map.explicit_pid_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0], (42, ProcessVisibility::All));
}

#[test]
fn child_inherits_parent_visibility() {
    // Our own PID's parent (the test runner) should be walkable.
    let our_pid = std::process::id();
    let parent_pid = read_ppid(our_pid).expect("should be able to read our PPid");

    let map = VisibilityMap::new(std::iter::empty());
    map.set_pid(parent_pid, ProcessVisibility::All);

    // Our PID has no direct override, but should inherit from parent.
    assert_eq!(map.resolve(our_pid), ProcessVisibility::All);

    // The result should now be cached as Cached (not Explicit).
    assert_eq!(
        map.pid_entries.read().get(&our_pid).copied(),
        Some(VisibilityEntry::Cached(ProcessVisibility::All))
    );
}

#[test]
fn cached_entry_does_not_propagate_to_children() {
    // A cached Default on a parent must NOT be inherited by children.
    // This was the root cause of git seeing VFS files when spawned by
    // a Default-visibility parent (e.g., Claude Code).
    let our_pid = std::process::id();
    let parent_pid = read_ppid(our_pid).expect("should be able to read our PPid");

    let map = VisibilityMap::new(std::iter::empty());

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

#[test]
fn ancestor_walk_stops_at_init() {
    // PID 1 (init) has no override — should not inherit anything.
    let map = VisibilityMap::new(std::iter::empty());
    assert_eq!(map.resolve(1), ProcessVisibility::Default);
}

#[test]
fn read_ppid_returns_valid_parent() {
    // Sanity: our own parent PID should be > 0.
    assert!(read_ppid(std::process::id()).is_some_and(|p| p > 0));
}

#[test]
fn read_ppid_returns_none_for_nonexistent() {
    assert_eq!(read_ppid(u32::MAX), None);
}
