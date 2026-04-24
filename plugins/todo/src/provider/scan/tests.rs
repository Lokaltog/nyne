use rstest::rstest;

use super::*;

/// Tests that `strip_tag_prefix` only accepts colon-terminated tags (optionally after `(author)`),
/// returning the trimmed remainder or `None` for bare/space/dash-separated forms.
#[rstest]
#[case::colon("TODO: fix this", "TODO", Some("fix this"))]
#[case::paren_author("TODO(user): fix this", "TODO", Some("fix this"))]
#[case::dash_rejected("FIXME - urgent", "FIXME", None)]
#[case::bare_space_rejected("HACK use a real parser", "HACK", None)]
#[case::bare_colon("TODO:", "TODO", Some(""))]
fn strip_tag_prefix_cases(#[case] line: &str, #[case] tag: &str, #[case] expected: Option<&str>) {
    assert_eq!(strip_tag_prefix(line, tag), expected.map(str::to_owned));
}

/// Tests byte-offset-to-line-number conversion over a canonical 3-line source.
#[rstest]
#[case::line0_start(0, 0)]
#[case::line0_newline(5, 0)]
#[case::line1_start(6, 1)]
#[case::line2_start(12, 2)]
fn byte_to_line_basic(#[case] offset: usize, #[case] expected: usize) {
    let starts = build_line_starts("line0\nline1\nline2\n");
    assert_eq!(byte_to_line(&starts, offset), expected);
}

/// Tests that `find_comment_block` correctly extracts comment ranges for both
/// line-comment groups and single block comments.
#[rstest]
#[case::line_comments(
    "fn foo() {}\n// TODO: first\n// continuation\nfn bar() {}\n",
    &["TODO: first", "continuation"],
)]
#[case::block_comment(
    "/* TODO: fix\n   this thing */\nfn foo() {}\n",
    &["/* TODO: fix", "this thing */"],
)]
fn find_comment_block_cases(#[case] source: &str, #[case] expected_substrings: &[&str]) {
    let line_starts = build_line_starts(source);
    let todo_offset = source.find("TODO").unwrap();
    let block = find_comment_block(source, &line_starts, todo_offset).unwrap();
    let block_text = &source[block];
    for expected in expected_substrings {
        assert!(
            block_text.contains(expected),
            "block {block_text:?} missing {expected:?}"
        );
    }
}
