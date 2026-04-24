use std::path::PathBuf;

use rstest::rstest;

use super::*;

/// Tests exact-output slugification: alphanumeric, special chars, empty, whitespace.
#[rstest]
#[case::basic("fix the frobnicator", "fix-the-frobnicator")]
#[case::special_chars("handle error (URGENT!)", "handle-error-urgent")]
#[case::empty("", "unnamed")]
#[case::whitespace_only("   ", "unnamed")]
fn slugify_exact(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(slugify_content(input), expected);
}

/// Tests that long content is truncated at a word boundary.
#[rstest]
fn slugify_truncation() {
    let long = "this is a very long comment that should be truncated at a word boundary";
    let slug = slugify_content(long);
    assert!(slug.len() <= 40);
    assert!(!slug.ends_with('-'));
}

fn entry(source: &str, line: usize, tag: &str, text: &str) -> Entry {
    Entry {
        source_file: PathBuf::from(source),
        line,
        tag: Arc::from(tag),
        text: text.to_owned(),
    }
}
/// Tests the filesystem-safe name format for todo entries.
#[rstest]
fn fs_name_format() {
    assert_eq!(
        entry("src/main.rs", 42, "TODO", "fix frobnicator").fs_name(),
        "src__main.rs:42--fix-frobnicator"
    );
}

/// Symlink from `@/todo/<tag>/<entry>` must reach the source file's companion symbol.
/// Paths are mount-root-relative: base includes the `@` companion prefix. The relative
/// target walks up 3 levels regardless of source file nesting — covers root-level and
/// nested paths uniformly.
#[rstest]
#[case::nested(
    "src/dispatch/router.rs",
    10,
    "FIXME",
    "null check",
    "../../../src/dispatch/router.rs@/symbols/at-line/10"
)]
#[case::root("ROADMAP.md", 788, "TODO", "hack", "../../../ROADMAP.md@/symbols/at-line/788")]
fn symlink_target(
    #[case] source_file: &str,
    #[case] line: usize,
    #[case] tag: &str,
    #[case] text: &str,
    #[case] expected: &str,
) {
    let entry = entry(source_file, line, tag, text);
    let companion = nyne_companion::Companion::new(None, "@".into());
    let source_paths = SourcePaths::default();
    assert_eq!(
        entry.symlink_target(&companion, "todo", &source_paths),
        PathBuf::from(expected)
    );
}
