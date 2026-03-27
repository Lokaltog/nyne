use std::path::PathBuf;

use super::*;

/// Tests that basic alphanumeric content is slugified correctly.
#[test]
fn slugify_basic() {
    assert_eq!(slugify_content("fix the frobnicator"), "fix-the-frobnicator");
}

/// Tests that special characters are stripped during slugification.
#[test]
fn slugify_special_chars() {
    assert_eq!(slugify_content("handle error (URGENT!)"), "handle-error-urgent");
}

/// Tests that long content is truncated at a word boundary.
#[test]
fn slugify_truncation() {
    let long = "this is a very long comment that should be truncated at a word boundary";
    let slug = slugify_content(long);
    assert!(slug.len() <= 40);
    assert!(!slug.ends_with('-'));
}

/// Tests that empty or whitespace-only content produces "unnamed".
#[test]
fn slugify_empty() {
    assert_eq!(slugify_content(""), "unnamed");
    assert_eq!(slugify_content("   "), "unnamed");
}

/// Tests the filesystem-safe name format for todo entries.
#[test]
fn fs_name_format() {
    let entry = Entry {
        source_file: VfsPath::new("src/main.rs").unwrap(),
        line: 42,
        tag: Arc::from("TODO"),
        text: "fix frobnicator".to_owned(),
    };
    assert_eq!(entry.fs_name(), "src__main.rs:42--fix-frobnicator");
}

/// Symlink from `@/todo/FIXME/<entry>` must reach `src/dispatch/router.rs@/symbols/at-line/10`.
/// Paths are mount-root-relative: base includes the `@` companion prefix.
#[test]
fn symlink_target_nested_source_file() {
    let entry = Entry {
        source_file: VfsPath::new("src/dispatch/router.rs").unwrap(),
        line: 10,
        tag: Arc::from("FIXME"),
        text: "null check".to_owned(),
    };
    // From @/todo/FIXME/ (3 levels) up to mount root, then to target companion.
    assert_eq!(
        entry.symlink_target(),
        PathBuf::from("../../../src/dispatch/router.rs@/symbols/at-line/10")
    );
}

/// Same test with a root-level file — ensures depth computation works at all levels.
#[test]
fn symlink_target_root_source_file() {
    let entry = Entry {
        source_file: VfsPath::new("ROADMAP.md").unwrap(),
        line: 788,
        tag: Arc::from("TODO"),
        text: "hack".to_owned(),
    };
    assert_eq!(
        entry.symlink_target(),
        PathBuf::from("../../../ROADMAP.md@/symbols/at-line/788")
    );
}
