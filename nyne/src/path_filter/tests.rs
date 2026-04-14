use std::fs;
use std::path::Path;

use rstest::rstest;
use tempfile::TempDir;

use super::*;

/// Write `contents` to `<root>/<rel>`, creating parent directories as
/// needed. Test helper — panics on any I/O error.
fn write_file(root: &Path, rel: &str, contents: &[u8]) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parents");
    }
    fs::write(path, contents).expect("write file");
}

/// Build a filter rooted in a temp directory seeded with an optional
/// root `.gitignore`, a list of files to touch, and optional
/// `excluded_patterns`. Returns the `TempDir` so the caller keeps it
/// alive for the duration of the test.
fn setup(gitignore: Option<&str>, files: &[&str], excluded: &[&str]) -> (TempDir, PathFilter) {
    let tmp = TempDir::new().expect("tempdir");
    if let Some(contents) = gitignore {
        write_file(tmp.path(), ".gitignore", contents.as_bytes());
    }
    for rel in files {
        write_file(tmp.path(), rel, b"");
    }
    let filter = PathFilter::build(
        tmp.path(),
        &excluded.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
    );
    (tmp, filter)
}

#[test]
fn empty_filter_excludes_nothing() {
    let filter = PathFilter::empty();
    assert!(!filter.is_excluded(Path::new("node_modules")));
    assert!(!filter.is_excluded(Path::new("node_modules/foo/bar.js")));
    assert!(!filter.is_excluded(Path::new("src/main.rs")));
}

#[rstest]
#[case::dir_pattern_root("node_modules")]
#[case::dir_pattern_nested_file("node_modules/foo/bar.js")]
#[case::dir_pattern_nested_dir("node_modules/foo")]
#[case::second_pattern("target/debug/app")]
#[case::first_pattern("node_modules/pkg")]
fn excludes_matching_paths(#[case] query: &str) {
    let (_tmp, filter) = setup(
        Some("node_modules/\ntarget/\n"),
        &["src/main.rs", "node_modules/foo/bar.js", "target/debug/app"],
        &[],
    );
    assert!(filter.is_excluded(Path::new(query)));
}

#[rstest]
#[case::unignored_sibling("src/main.rs")]
#[case::unignored_lookalike("node_modules_alt")]
fn permits_unmatched_paths(#[case] query: &str) {
    let (_tmp, filter) = setup(
        Some("node_modules/\ntarget/\n"),
        &[
            "src/main.rs",
            "node_modules/foo/bar.js",
            "target/debug/app",
            "node_modules_alt",
        ],
        &[],
    );
    assert!(!filter.is_excluded(Path::new(query)));
}

#[rstest]
#[case::bare_suffix("node_modules/foo/bar.js@")]
#[case::deeply_wrapped("node_modules/foo/bar.js@/symbols/Foo@/body.rs")]
fn companion_wrapped_paths_under_ignored_dir_are_excluded(#[case] query: &str) {
    // Regression: the original bug. Companion-wrapped paths inside a
    // gitignored directory must be recognised as excluded because
    // `matched_path_or_any_parents` walks ancestor components from leaf
    // to root and hits `node_modules` before ever touching the
    // `@`-suffixed leaf.
    let (_tmp, filter) = setup(Some("node_modules/\n"), &["node_modules/foo/bar.js"], &[]);
    assert!(filter.is_excluded(Path::new(query)));
}

#[rstest]
#[case::dir_match("custom_dir")]
#[case::file_under_dir("custom_dir/file")]
fn excluded_patterns_applied_without_gitignore(#[case] query: &str) {
    let (_tmp, filter) = setup(None, &["custom_dir/file"], &["custom_dir/"]);
    assert!(filter.is_excluded(Path::new(query)));
}

#[rstest]
#[case::gitignore_hit("node_modules/foo")]
#[case::excluded_patterns_hit("vendor/lib")]
fn excluded_patterns_compose_with_gitignore(#[case] query: &str) {
    let (_tmp, filter) = setup(
        Some("node_modules/\n"),
        &["node_modules/foo", "vendor/lib", "src/main.rs"],
        &["vendor/"],
    );
    assert!(filter.is_excluded(Path::new(query)));
}

#[test]
fn excluded_patterns_do_not_match_unrelated_paths() {
    let (_tmp, filter) = setup(
        Some("node_modules/\n"),
        &["node_modules/foo", "vendor/lib", "src/main.rs"],
        &["vendor/"],
    );
    assert!(!filter.is_excluded(Path::new("src/main.rs")));
}

#[test]
fn nested_gitignore_scopes_to_its_own_subtree() {
    // A `.gitignore` at `a/` only ignores paths under `a/`, not the
    // sibling `b/` subtree with the same file layout. Pattern name is
    // deliberately obscure so it can't collide with any realistic
    // user-level global gitignore the test environment picks up.
    let tmp = TempDir::new().unwrap();
    for rel in ["a/nyne_scope_fixture/out.bin", "b/nyne_scope_fixture/out.bin"] {
        write_file(tmp.path(), rel, b"");
    }
    write_file(tmp.path(), "a/.gitignore", b"nyne_scope_fixture/\n");
    let filter = PathFilter::build(tmp.path(), &[]);
    assert!(filter.is_excluded(Path::new("a/nyne_scope_fixture/out.bin")));
    assert!(!filter.is_excluded(Path::new("b/nyne_scope_fixture/out.bin")));
}

#[test]
fn absolute_path_under_source_root_is_matched() {
    let (tmp, filter) = setup(Some("node_modules/\n"), &["node_modules/foo"], &[]);
    let abs = tmp.path().join("node_modules/foo");
    assert!(filter.is_excluded(&abs));
}

#[rstest]
#[case::empty("")]
#[case::root("/")]
fn sentinel_paths_are_never_excluded(#[case] query: &str) {
    let (_tmp, filter) = setup(Some("node_modules/\n"), &[], &[]);
    assert!(!filter.is_excluded(Path::new(query)));
}

#[test]
fn whitelist_rescues_file_under_ignored_dir() {
    // `matched_path_or_any_parents` checks the leaf path against the
    // rules before walking ancestors. A `!build/keep.txt` negation
    // therefore rescues the file even though `build/` itself is
    // ignored, while unrelated siblings stay excluded.
    let (_tmp, filter) = setup(
        Some("build/\n!build/keep.txt\n"),
        &["build/keep.txt", "build/drop.o"],
        &[],
    );
    assert!(!filter.is_excluded(Path::new("build/keep.txt")));
    assert!(filter.is_excluded(Path::new("build/drop.o")));
}

#[test]
fn invalid_excluded_pattern_is_skipped_not_fatal() {
    // One malformed pattern, one valid — build succeeds and the valid
    // pattern still matches.
    let (_tmp, filter) = setup(None, &["vendor/lib"], &["[", "vendor/"]);
    assert!(filter.is_excluded(Path::new("vendor/lib")));
}
