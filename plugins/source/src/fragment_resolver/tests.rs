use std::path::PathBuf;
use std::sync::Arc;

use rstest::rstest;

use super::*;
use crate::syntax::decomposed::DecompositionCache;
use crate::test_support::registry;

/// Build a `FragmentResolver` for a Rust source file written to a tempdir.
fn resolver_for_source(dir: &std::path::Path, source: &str) -> FragmentResolver {
    std::fs::write(dir.join("test.rs"), source).unwrap();
    let fs: Arc<dyn nyne::router::Filesystem> = Arc::new(nyne::router::fs::os::OsFilesystem::new(dir));
    let cache = DecompositionCache::new(fs, Arc::new(registry()), 5);
    FragmentResolver::new(cache, PathBuf::from("test.rs"))
}

/// Tests that `line_range` returns the correct 1-based line for existing symbols
/// and `None` for nonexistent ones.
#[rstest]
#[case::first_symbol("fn hello() {}\n\nfn world() {}\n", "hello", Some(1))]
#[case::later_symbol("fn hello() {}\n\nfn world() {}\n", "world", Some(3))]
#[case::missing_symbol("fn hello() {}\n", "nonexistent", None)]
fn line_range_cases(#[case] source: &str, #[case] symbol: &str, #[case] expected_start: Option<usize>) {
    let dir = tempfile::tempdir().unwrap();
    let resolver = resolver_for_source(dir.path(), source);
    let range = resolver.line_range(&[symbol.into()]).unwrap();
    assert_eq!(range.map(|r| r.start), expected_start);
}

/// Tests that `line_range` reflects source changes after cache invalidation.
#[rstest]
fn line_range_reflects_source_change_after_invalidation() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.rs");
    let resolver = resolver_for_source(dir.path(), "fn hello() {}\n");
    let hello_start = || resolver.line_range(&["hello".into()]).unwrap().map(|r| r.start);

    assert_eq!(hello_start(), Some(1), "initial: hello on line 1");

    // Modify source: insert a blank line + new function before hello.
    std::fs::write(&file, "fn first() {}\n\nfn hello() {}\n").unwrap();
    resolver.invalidate();

    assert_eq!(hello_start(), Some(3), "after invalidation: hello moved to line 3");
}
