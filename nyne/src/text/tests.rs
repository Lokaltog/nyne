use rstest::rstest;

use super::*;

/// Verifies slug generation with truncation and edge cases.
#[rstest]
#[case::basic("Fix the bug", 50, "fix-the-bug")]
#[case::special_chars("feat(scope): add thing!", 50, "feat-scope-add-thing")]
#[case::truncates_at_hyphen_boundary("this is a very long commit message", 20, "this-is-a-very-long")]
#[case::no_leading_or_trailing_hyphens("--hello--world--", 50, "hello-world")]
#[case::empty("", 50, "")]
fn slugify_conversion(#[case] input: &str, #[case] max_len: usize, #[case] expected: &str) {
    assert_eq!(slugify(input, max_len), expected);
}

/// Verifies git date formatting for known timestamps.
#[rstest]
#[case::epoch(0, "1970-01-01")]
#[case::known_date(1_705_276_800, "2024-01-15")]
fn format_git_date_cases(#[case] timestamp: i64, #[case] expected: &str) {
    assert_eq!(format_git_date(timestamp), expected);
}

/// Unified diff correctly shows a replaced line.
#[test]
fn unified_diff_replacement() {
    let old = "line one\nline two\nline three\n";
    let new = "line one\nline TWO\nline three\n";
    insta::assert_snapshot!(unified_diff(old, new, "src/foo.rs"));
}

/// Unified diff correctly shows an inserted line.
#[test]
fn unified_diff_insertion() {
    let old = "first\nsecond\n";
    let new = "first\ninserted\nsecond\n";
    insta::assert_snapshot!(unified_diff(old, new, "src/foo.rs"));
}

/// Unified diff correctly shows a deleted line.
#[test]
fn unified_diff_deletion() {
    let old = "keep\nremove\ntrailing\n";
    let new = "keep\ntrailing\n";
    insta::assert_snapshot!(unified_diff(old, new, "src/foo.rs"));
}

/// Unified diff produces multiple hunks for non-adjacent changes.
#[test]
fn unified_diff_multi_hunk() {
    let old = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n";
    let new = "A\nb\nc\nd\ne\nf\ng\nh\ni\nJ\n";
    insta::assert_snapshot!(unified_diff(old, new, "src/foo.rs"));
}

/// Identical inputs produce an empty diff.
#[test]
fn unified_diff_identical_returns_empty() {
    let content = "unchanged\n";
    assert!(unified_diff(content, content, "src/foo.rs").is_empty());
}
