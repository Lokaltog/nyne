use super::*;
use crate::test_support::vfs as vpath;

/// Tests that `invalidate_subtree` marks both direct and nested directories as unresolved.
#[test]
fn invalidate_subtree_marks_nested_dirs_unresolved() {
    let cache = L1Cache::new();

    // Simulate resolved directories at edit/ and edit/staged/.
    let edit = vpath("src/foo.rs@/symbols/Bar@/edit");
    let staged = vpath("src/foo.rs@/symbols/Bar@/edit/staged");

    // Mark both as resolved.
    cache.get_or_create(&edit).write().begin_resolve();
    cache.get_or_create(&staged).write().begin_resolve();

    assert!(cache.get(&edit).unwrap().read().is_resolved());
    assert!(cache.get(&staged).unwrap().read().is_resolved());

    // Invalidate the subtree at edit/.
    cache.invalidate_subtree(&edit);

    // Both should be marked unresolved.
    assert!(
        !cache.get(&edit).unwrap().read().is_resolved(),
        "edit/ should be unresolved"
    );
    assert!(
        !cache.get(&staged).unwrap().read().is_resolved(),
        "edit/staged/ should be unresolved"
    );
}

/// Create a virtual `CachedNode` for testing with the given parameters.
fn make_node(name: &str, inode: u64, resolve_gen: u64, source: NodeSource) -> (String, CachedNode) {
    let pid = ProviderId::new("test");
    (name.to_owned(), CachedNode {
        inode,
        kind: CachedNodeKind::Virtual {
            provider_id: pid,
            node: Arc::new(VirtualNode::file(name, crate::node::builtins::StaticContent(b""))),
        },
        source,
        generation: resolve_gen,
    })
}

/// Verifies that after subtree invalidation, re-resolution can insert new entries.
#[test]
fn invalidate_subtree_allows_re_resolve_with_new_entries() {
    let cache = L1Cache::new();
    let staged = vpath("src/foo.rs@/symbols/Bar@/edit/staged");

    // First resolve — insert one entry.
    {
        let handle = cache.get_or_create(&staged);
        let mut dir = handle.write();
        dir.begin_resolve();
        let generation = dir.resolve_generation();
        let (name, node) = make_node("10-replace.diff", 100, generation, NodeSource::Children);
        dir.insert(name, node);
    }

    assert_eq!(
        cache
            .get(&staged)
            .unwrap()
            .read()
            .readdir_entries(ProcessVisibility::Default)
            .count(),
        1
    );
    assert!(cache.get(&staged).unwrap().read().is_resolved());

    // Invalidate the parent subtree.
    cache.invalidate_subtree(&vpath("src/foo.rs@/symbols/Bar@/edit"));

    assert!(!cache.get(&staged).unwrap().read().is_resolved());

    // Second resolve — insert two entries.
    {
        let handle = cache.get_or_create(&staged);
        let mut dir = handle.write();
        dir.begin_resolve();
        let generation = dir.resolve_generation();
        let (n1, cn1) = make_node("10-replace.diff", 100, generation, NodeSource::Children);
        let (n2, cn2) = make_node("20-insert_after.diff", 101, generation, NodeSource::Children);
        dir.insert(n1, cn1);
        dir.insert(n2, cn2);
        let swept = dir.sweep_stale_resolve(generation);
        assert!(swept.is_empty(), "no entries should be swept");
    }

    let handle = cache.get(&staged).unwrap();
    let dir = handle.read();
    let entries: Vec<_> = dir
        .readdir_entries(ProcessVisibility::Default)
        .map(|(n, _)| n.to_owned())
        .collect();
    assert_eq!(
        entries.len(),
        2,
        "should have 2 entries after re-resolve, got: {entries:?}"
    );
}

/// Tests that `collect_dir_inodes_under` finds nested directories but not unrelated paths.
#[test]
fn collect_dir_inodes_under_finds_nested_dirs() {
    let cache = L1Cache::new();

    let edit = vpath("src/foo.rs@/symbols/Bar@/edit");
    let staged = vpath("src/foo.rs@/symbols/Bar@/edit/staged");
    let unrelated = vpath("src/other.rs@/symbols/Baz@/edit");

    cache.get_or_create(&edit).write().begin_resolve();
    cache.get_or_create(&staged).write().begin_resolve();
    cache.get_or_create(&unrelated).write().begin_resolve();

    // Assign fake inodes via the callback — just use the path length as a stand-in.
    let inodes = cache.collect_dir_inodes_under(&edit, |p| p.as_str().len() as u64);

    // Should find edit/ and edit/staged/, but not unrelated.
    assert_eq!(inodes.len(), 2, "should find 2 directories under edit/");
    assert!(inodes.contains(&(edit.as_str().len() as u64)));
    assert!(inodes.contains(&(staged.as_str().len() as u64)));
}

/// Create a real filesystem `CachedNode` for testing.
fn make_real_node(name: &str, source: NodeSource, inode: u64) -> (String, CachedNode) {
    (name.to_owned(), CachedNode {
        inode,
        kind: CachedNodeKind::Real {
            file_type: crate::types::file_kind::FileKind::File,
        },
        source,
        generation: 0,
    })
}

/// Tests that virtual nodes with default `Readdir` visibility are visible.
#[test]
fn is_visible_virtual_readdir() {
    let (_, cn) = make_node("a.rs", 1, 1, NodeSource::Children);
    assert!(cn.is_visible(), "default Visibility::Readdir should be visible");
}

/// Tests that virtual nodes with `Hidden` visibility are not visible.
#[test]
fn is_visible_virtual_hidden() {
    let pid = ProviderId::new("test");
    let cn = CachedNode {
        inode: 1,
        kind: CachedNodeKind::Virtual {
            provider_id: pid,
            node: Arc::new(VirtualNode::file("hidden", crate::node::builtins::StaticContent(b"")).hidden()),
        },
        source: NodeSource::Children,
        generation: 1,
    };
    assert!(!cn.is_visible(), "Visibility::Hidden should not be visible");
}

/// Verifies that derived nodes respect their visibility setting.
#[test]
fn is_visible_derived_respects_visibility() {
    let (_, cn) = make_node("derived.rs", 1, 1, NodeSource::Derived);
    assert!(cn.is_visible(), "derived node with default Readdir should be visible");

    let pid = ProviderId::new("test");
    let cn = CachedNode {
        inode: 2,
        kind: CachedNodeKind::Virtual {
            provider_id: pid,
            node: Arc::new(VirtualNode::file("hidden", crate::node::builtins::StaticContent(b"")).hidden()),
        },
        source: NodeSource::Derived,
        generation: 1,
    };
    assert!(!cn.is_visible(), "derived node with Hidden should not be visible");
}

/// Tests that mutated (user-created) nodes are always visible regardless of settings.
#[test]
fn is_visible_mutated_always_visible() {
    let (_, cn) = make_node("mutated", 1, 0, NodeSource::Mutated);
    assert!(cn.is_visible(), "mutated nodes are always visible");
}

/// Tests that real filesystem nodes are always visible.
#[test]
fn is_visible_real_always_visible() {
    let (_, cn) = make_real_node("real.txt", NodeSource::Lookup, 1);
    assert!(cn.is_visible(), "real nodes are always visible");
}

/// Tests that stale children and derived entries are swept while lookup entries survive.
#[test]
fn sweep_stale_resolve_sweeps_derived_entries() {
    let mut dir = DirState::new();
    dir.begin_resolve();
    let generation = dir.resolve_generation();

    let (name, cn) = make_node("children.rs", 10, generation, NodeSource::Children);
    dir.insert(name, cn);

    let (name, cn) = make_node("derived.rs", 20, generation, NodeSource::Derived);
    dir.insert(name, cn);

    let (name, cn) = make_node("lookup.rs", 30, 0, NodeSource::Lookup);
    dir.insert(name, cn);

    // New generation — children and derived from old generation should be swept.
    dir.begin_resolve();
    let swept = dir.sweep_stale_resolve(dir.resolve_generation());

    assert_eq!(swept.len(), 2, "both children and derived should be swept");
    assert!(dir.get("children.rs").is_none());
    assert!(dir.get("derived.rs").is_none());
    assert!(dir.get("lookup.rs").is_some(), "lookup entries survive sweep");
}

/// Verifies that directories without a source file are never considered stale.
#[test]
fn is_source_stale_false_when_no_source() {
    let dir = DirState::new();
    assert!(!dir.is_source_stale(|_| 99), "no source → never stale");
}

/// Verifies that a directory is not stale when its generation matches the source.
#[test]
fn is_source_stale_false_when_generation_matches() {
    let mut dir = DirState::new();
    dir.set_source_generation(VfsPath::new("src/lib.rs").unwrap(), 5);
    assert!(!dir.is_source_stale(|_| 5));
}

/// Verifies that a directory is stale when the source generation has advanced.
#[test]
fn is_source_stale_true_when_generation_advanced() {
    let mut dir = DirState::new();
    dir.set_source_generation(VfsPath::new("src/lib.rs").unwrap(), 5);
    assert!(dir.is_source_stale(|_| 6));
}
