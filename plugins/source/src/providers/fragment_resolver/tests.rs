use std::sync::Arc;

use nyne::types::OsFs;
use nyne::types::vfs_path::VfsPath;

use super::*;
use crate::syntax::decomposed::DecompositionCache;
use crate::test_support::registry;

/// Build a `FragmentResolver` for a Rust source file written to a tempdir.
fn resolver_for_source(dir: &std::path::Path, source: &str) -> FragmentResolver {
    let file = dir.join("test.rs");
    std::fs::write(&file, source).unwrap();
    let real_fs: Arc<dyn nyne::types::RealFs> = Arc::new(OsFs::new(dir.to_path_buf()));
    let cache = DecompositionCache::new(real_fs, Arc::new(registry()));
    let vfs_path = VfsPath::new("test.rs").unwrap();
    FragmentResolver::new(cache, vfs_path)
}

/// Tests that line_range returns a valid range for a top-level symbol.
#[test]
fn line_range_returns_range_for_top_level_symbol() {
    let dir = tempfile::tempdir().unwrap();
    let source = "fn hello() {}\n\nfn world() {}\n";
    let resolver = resolver_for_source(dir.path(), source);

    let range = resolver.line_range(&["hello".into()]).unwrap();
    assert!(range.is_some(), "expected Some for existing symbol");
    let range = range.unwrap();
    assert_eq!(range.start, 1, "hello is on line 1 (1-based)");

    let range = resolver.line_range(&["world".into()]).unwrap();
    let range = range.unwrap();
    assert_eq!(range.start, 3, "world is on line 3 (1-based)");
}

/// Tests that line_range returns None for a nonexistent symbol.
#[test]
fn line_range_returns_none_for_missing_symbol() {
    let dir = tempfile::tempdir().unwrap();
    let resolver = resolver_for_source(dir.path(), "fn hello() {}\n");

    let range = resolver.line_range(&["nonexistent".into()]).unwrap();
    assert!(range.is_none());
}

/// Tests that line_range reflects source changes after cache invalidation.
#[test]
fn line_range_reflects_source_change_after_invalidation() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.rs");
    let resolver = resolver_for_source(dir.path(), "fn hello() {}\n");

    // Initial: hello on line 1.
    let range = resolver.line_range(&["hello".into()]).unwrap().unwrap();
    assert_eq!(range.start, 1);

    // Modify source: insert a blank line + new function before hello.
    std::fs::write(&file, "fn first() {}\n\nfn hello() {}\n").unwrap();
    resolver.invalidate();

    // After invalidation: hello moved to line 3.
    let range = resolver.line_range(&["hello".into()]).unwrap().unwrap();
    assert_eq!(range.start, 3, "hello should move to line 3 after source change");
}
