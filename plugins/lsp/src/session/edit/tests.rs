use std::collections::HashMap;
use std::io::Write as _;

use lsp_types::{Position, Range, TextEdit, WorkspaceEdit};
use rstest::rstest;

use super::*;

/// Build an `PathResolver` that passes paths through unchanged.
///
/// Tests use temp files outside any project root, so rewriting is a no-op.
fn passthrough_resolver() -> crate::session::path::PathResolver {
    crate::session::path::PathResolver::new("/nonexistent-root".into(), "/nonexistent-root".into())
}

/// Helper: create an `lsp_types::Uri` from a filesystem path string.
fn uri_from_path(path: &str) -> lsp_types::Uri {
    let url = url::Url::from_file_path(path).expect("valid absolute path");
    url.as_str().parse().expect("valid URI")
}

/// Helper: build a `TextEdit` from line/col ranges and replacement text.
fn text_edit(start_line: u32, start_col: u32, end_line: u32, end_col: u32, new_text: &str) -> TextEdit {
    TextEdit {
        range: Range {
            start: Position {
                line: start_line,
                character: start_col,
            },
            end: Position {
                line: end_line,
                character: end_col,
            },
        },
        new_text: new_text.to_owned(),
    }
}

/// Helper: build a `WorkspaceEdit` with simple `changes` map.
fn workspace_edit(entries: Vec<(lsp_types::Uri, Vec<TextEdit>)>) -> WorkspaceEdit {
    WorkspaceEdit {
        changes: Some(entries.into_iter().collect()),
        document_changes: None,
        change_annotations: None,
    }
}

/// Helper: create a temp file with the given content and return `(handle, path_str)`.
///
/// The handle must stay in scope for the test — dropping it deletes the file.
fn tempfile_with(content: &str) -> (tempfile::NamedTempFile, String) {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    tmp.write_all(content.as_bytes()).unwrap();
    tmp.flush().unwrap();
    let path = tmp.path().to_str().unwrap().to_owned();
    (tmp, path)
}

/// Helper: build a `WorkspaceEdit` targeting a single file with the given edits.
fn single_file_edit(path: &str, edits: Vec<TextEdit>) -> WorkspaceEdit {
    workspace_edit(vec![(uri_from_path(path), edits)])
}

/// Helper: build an empty `WorkspaceEdit` (no file changes).
fn empty_workspace_edit() -> WorkspaceEdit {
    WorkspaceEdit {
        changes: Some(HashMap::new()),
        document_changes: None,
        change_annotations: None,
    }
}

/// Verifies that [`apply_workspace_edit`] applied to a single-file edit produces
/// the expected final file content across every supported edit shape.
#[rstest]
#[case::single_edit(
    "hello world",
    vec![text_edit(0, 0, 0, 5, "goodbye")],
    "goodbye world",
)]
#[case::multiple_edits(
    "aaa bbb ccc",
    vec![text_edit(0, 0, 0, 3, "xxx"), text_edit(0, 8, 0, 11, "zzz")],
    "xxx bbb zzz",
)]
#[case::multiline(
    "line one\nline two\nline three",
    vec![text_edit(1, 5, 1, 8, "TWO"), text_edit(2, 5, 2, 10, "THREE")],
    "line one\nline TWO\nline THREE",
)]
#[case::insert_at_position("ab", vec![text_edit(0, 1, 0, 1, "X")], "aXb")]
#[case::delete_range("hello world", vec![text_edit(0, 5, 0, 11, "")], "hello")]
// 'a' + emoji (U+1F600, 4 bytes UTF-8, 2 UTF-16 code units) + 'b'.
// Replace 'b' which sits at UTF-16 offset 3 (after 'a'=1 + emoji=2).
#[case::utf16_surrogate(
    "a\u{1F600}b",
    vec![text_edit(0, 3, 0, 4, "Z")],
    "a\u{1F600}Z",
)]
fn apply_workspace_edit_single_file(#[case] content: &str, #[case] edits: Vec<TextEdit>, #[case] expected: &str) {
    let (_tmp, path) = tempfile_with(content);
    let edit = single_file_edit(&path, edits);
    apply_workspace_edit(&edit, &passthrough_resolver()).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
}

/// Tests applying edits across multiple files simultaneously.
#[rstest]
fn multiple_files() {
    let (_tmp1, path1) = tempfile_with("file one");
    let (_tmp2, path2) = tempfile_with("file two");

    let edit = workspace_edit(vec![
        (uri_from_path(&path1), vec![text_edit(0, 5, 0, 8, "ONE")]),
        (uri_from_path(&path2), vec![text_edit(0, 5, 0, 8, "TWO")]),
    ]);

    apply_workspace_edit(&edit, &passthrough_resolver()).unwrap();
    assert_eq!(std::fs::read_to_string(&path1).unwrap(), "file ONE");
    assert_eq!(std::fs::read_to_string(&path2).unwrap(), "file TWO");
}

/// Tests that an empty workspace edit succeeds without error.
#[rstest]
fn empty_edit_warns_but_succeeds() {
    // Should succeed (no files to modify), just emits a warning.
    apply_workspace_edit(&empty_workspace_edit(), &passthrough_resolver()).unwrap();
}

/// Verifies [`apply_edits_to_rope`] applied to in-memory content produces the
/// expected resulting string across the full range of edit shapes. `None` in
/// `expected` means the call should error (out of range).
#[rstest]
#[case::single_replacement("hello world", vec![text_edit(0, 0, 0, 5, "goodbye")], Some("goodbye world"))]
#[case::multiple_reverse_order(
    "aaa bbb ccc",
    vec![text_edit(0, 0, 0, 3, "xxx"), text_edit(0, 8, 0, 11, "zzz")],
    Some("xxx bbb zzz"),
)]
#[case::insertion("ab", vec![text_edit(0, 1, 0, 1, "X")], Some("aXb"))]
#[case::deletion("hello world", vec![text_edit(0, 5, 0, 11, "")], Some("hello"))]
#[case::multiline(
    "line one\nline two\nline three",
    vec![text_edit(1, 5, 1, 8, "TWO"), text_edit(2, 5, 2, 10, "THREE")],
    Some("line one\nline TWO\nline THREE"),
)]
#[case::out_of_range_errors("short", vec![text_edit(5, 0, 5, 1, "x")], None)]
fn apply_edits_to_rope_cases(
    #[case] content: &str,
    #[case] edits: Vec<TextEdit>,
    #[case] expected: Option<&str>,
) {
    let refs: Vec<&TextEdit> = edits.iter().collect();
    let mut edits_refs = refs;
    let result = apply_edits_to_rope(content, &mut edits_refs);
    match expected {
        Some(want) => assert_eq!(result.unwrap(), want),
        None => assert!(result.is_err()),
    }
}

/// Verifies [`resolve_edits`] applied to a single-file edit produces exactly
/// one result whose `modified` content matches expectations. When
/// `expected_original` is `Some`, it is also asserted verbatim.
#[rstest]
#[case::single_edit(
    "hello world\n",
    vec![text_edit(0, 0, 0, 5, "goodbye")],
    Some("hello world\n"),
    "goodbye world\n",
)]
#[case::multiple_edits(
    "aaa bbb ccc\n",
    vec![text_edit(0, 0, 0, 3, "xxx"), text_edit(0, 8, 0, 11, "zzz")],
    Some("aaa bbb ccc\n"),
    "xxx bbb zzz\n",
)]
#[case::insertion(
    "line one\nline two\n",
    vec![text_edit(1, 0, 1, 0, "inserted\n")],
    None,
    "line one\ninserted\nline two\n",
)]
#[case::deletion(
    "keep\nremove\ntrailing\n",
    vec![text_edit(1, 0, 2, 0, "")],
    None,
    "keep\ntrailing\n",
)]
fn resolve_edits_single_file(
    #[case] content: &str,
    #[case] edits: Vec<TextEdit>,
    #[case] expected_original: Option<&str>,
    #[case] expected_modified: &str,
) {
    let (_tmp, path) = tempfile_with(content);
    let edit = single_file_edit(&path, edits);
    let results = resolve_edits(&edit, &passthrough_resolver()).unwrap();
    assert_eq!(results.len(), 1);
    if let Some(original) = expected_original {
        assert_eq!(results[0].original, original);
    }
    assert_eq!(results[0].modified, expected_modified);
}

/// Tests that edits across multiple files resolve to per-file results.
#[rstest]
fn resolve_multiple_files() {
    let (_tmp1, path1) = tempfile_with("file one\n");
    let (_tmp2, path2) = tempfile_with("file two\n");

    let edit = workspace_edit(vec![
        (uri_from_path(&path1), vec![text_edit(0, 5, 0, 8, "ONE")]),
        (uri_from_path(&path2), vec![text_edit(0, 5, 0, 8, "TWO")]),
    ]);
    let mut results = resolve_edits(&edit, &passthrough_resolver()).unwrap();
    results.sort_by(|a, b| a.modified.cmp(&b.modified));

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].modified, "file ONE\n");
    assert_eq!(results[1].modified, "file TWO\n");
}

/// Tests that an empty workspace edit resolves to no results.
#[rstest]
fn resolve_empty_edit_returns_empty() {
    let results = resolve_edits(&empty_workspace_edit(), &passthrough_resolver()).unwrap();
    assert!(results.is_empty());
}

/// Tests that replacing text with identical content produces no effective change.
#[rstest]
fn resolve_no_change_returns_identical_content() {
    let (_tmp, path) = tempfile_with("unchanged\n");

    // Replace "unchanged" with "unchanged" — no actual change.
    let edit = single_file_edit(&path, vec![text_edit(0, 0, 0, 9, "unchanged")]);
    let results = resolve_edits(&edit, &passthrough_resolver()).unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].original, results[0].modified);
}
