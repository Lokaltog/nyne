use std::collections::HashMap;
use std::io::Write as _;

use lsp_types::{Position, Range, TextEdit, WorkspaceEdit};

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

/// Tests applying a single replacement to one file.
#[test]
fn single_file_single_edit() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "hello world").unwrap();
    tmp.flush().unwrap();
    let path = tmp.path().to_str().unwrap();

    let edit = workspace_edit(vec![(uri_from_path(path), vec![text_edit(0, 0, 0, 5, "goodbye")])]);

    apply_workspace_edit(&edit, &passthrough_resolver()).unwrap();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "goodbye world");
}

/// Tests applying multiple replacements to the same file.
#[test]
fn single_file_multiple_edits() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "aaa bbb ccc").unwrap();
    tmp.flush().unwrap();
    let path = tmp.path().to_str().unwrap();

    // Replace "aaa" with "xxx" and "ccc" with "zzz" in the same file.
    let edit = workspace_edit(vec![(uri_from_path(path), vec![
        text_edit(0, 0, 0, 3, "xxx"),
        text_edit(0, 8, 0, 11, "zzz"),
    ])]);

    apply_workspace_edit(&edit, &passthrough_resolver()).unwrap();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "xxx bbb zzz");
}

/// Tests applying edits across multiple files simultaneously.
#[test]
fn multiple_files() {
    let mut tmp1 = tempfile::NamedTempFile::new().unwrap();
    write!(tmp1, "file one").unwrap();
    tmp1.flush().unwrap();
    let path1 = tmp1.path().to_str().unwrap().to_owned();

    let mut tmp2 = tempfile::NamedTempFile::new().unwrap();
    write!(tmp2, "file two").unwrap();
    tmp2.flush().unwrap();
    let path2 = tmp2.path().to_str().unwrap().to_owned();

    let edit = workspace_edit(vec![
        (uri_from_path(&path1), vec![text_edit(0, 5, 0, 8, "ONE")]),
        (uri_from_path(&path2), vec![text_edit(0, 5, 0, 8, "TWO")]),
    ]);

    apply_workspace_edit(&edit, &passthrough_resolver()).unwrap();
    assert_eq!(std::fs::read_to_string(&path1).unwrap(), "file ONE");
    assert_eq!(std::fs::read_to_string(&path2).unwrap(), "file TWO");
}

/// Tests applying edits on different lines of the same file.
#[test]
fn multiline_edits() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "line one\nline two\nline three").unwrap();
    tmp.flush().unwrap();
    let path = tmp.path().to_str().unwrap();

    // Replace "two" on line 1 and "three" on line 2.
    let edit = workspace_edit(vec![(uri_from_path(path), vec![
        text_edit(1, 5, 1, 8, "TWO"),
        text_edit(2, 5, 2, 10, "THREE"),
    ])]);

    apply_workspace_edit(&edit, &passthrough_resolver()).unwrap();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "line one\nline TWO\nline THREE");
}

/// Tests inserting text at a position using an empty range.
#[test]
fn insert_at_position() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "ab").unwrap();
    tmp.flush().unwrap();
    let path = tmp.path().to_str().unwrap();

    // Insert "X" between "a" and "b" (empty range = insertion).
    let edit = workspace_edit(vec![(uri_from_path(path), vec![text_edit(0, 1, 0, 1, "X")])]);

    apply_workspace_edit(&edit, &passthrough_resolver()).unwrap();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "aXb");
}

/// Tests deleting a range by replacing it with empty text.
#[test]
fn delete_range() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "hello world").unwrap();
    tmp.flush().unwrap();
    let path = tmp.path().to_str().unwrap();

    // Delete " world" (empty new_text = deletion).
    let edit = workspace_edit(vec![(uri_from_path(path), vec![text_edit(0, 5, 0, 11, "")])]);

    apply_workspace_edit(&edit, &passthrough_resolver()).unwrap();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "hello");
}

/// Tests that an empty workspace edit succeeds without error.
#[test]
fn empty_edit_warns_but_succeeds() {
    let edit = WorkspaceEdit {
        changes: Some(HashMap::new()),
        document_changes: None,
        change_annotations: None,
    };

    // Should succeed (no files to modify), just emits a warning.
    apply_workspace_edit(&edit, &passthrough_resolver()).unwrap();
}

/// Tests that edits handle UTF-16 surrogate positions correctly.
#[test]
fn utf16_surrogate_positions() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    // 'a' + emoji (U+1F600, 4 bytes UTF-8, 2 code units UTF-16) + 'b'
    write!(tmp, "a\u{1F600}b").unwrap();
    tmp.flush().unwrap();
    let path = tmp.path().to_str().unwrap();

    // Replace 'b' which is at UTF-16 offset 3 (after 'a'=1 + emoji=2).
    let edit = workspace_edit(vec![(uri_from_path(path), vec![text_edit(0, 3, 0, 4, "Z")])]);

    apply_workspace_edit(&edit, &passthrough_resolver()).unwrap();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "a\u{1F600}Z");
}

/// Tests that a single rope replacement applies correctly.
#[test]
fn rope_single_replacement() {
    let content = "hello world";
    let edit = text_edit(0, 0, 0, 5, "goodbye");
    let mut edits = vec![&edit];
    let result = apply_edits_to_rope(content, &mut edits).unwrap();
    assert_eq!(result, "goodbye world");
}

/// Tests that multiple rope edits are applied in reverse order correctly.
#[test]
fn rope_multiple_edits_reverse_order() {
    let content = "aaa bbb ccc";
    let e1 = text_edit(0, 0, 0, 3, "xxx");
    let e2 = text_edit(0, 8, 0, 11, "zzz");
    let mut edits = vec![&e1, &e2];
    let result = apply_edits_to_rope(content, &mut edits).unwrap();
    assert_eq!(result, "xxx bbb zzz");
}

/// Tests rope insertion at a zero-width range.
#[test]
fn rope_insertion() {
    let content = "ab";
    let edit = text_edit(0, 1, 0, 1, "X");
    let mut edits = vec![&edit];
    let result = apply_edits_to_rope(content, &mut edits).unwrap();
    assert_eq!(result, "aXb");
}

/// Tests rope deletion with empty replacement text.
#[test]
fn rope_deletion() {
    let content = "hello world";
    let edit = text_edit(0, 5, 0, 11, "");
    let mut edits = vec![&edit];
    let result = apply_edits_to_rope(content, &mut edits).unwrap();
    assert_eq!(result, "hello");
}

/// Tests rope edits spanning multiple lines.
#[test]
fn rope_multiline() {
    let content = "line one\nline two\nline three";
    let e1 = text_edit(1, 5, 1, 8, "TWO");
    let e2 = text_edit(2, 5, 2, 10, "THREE");
    let mut edits = vec![&e1, &e2];
    let result = apply_edits_to_rope(content, &mut edits).unwrap();
    assert_eq!(result, "line one\nline TWO\nline THREE");
}

/// Tests that an out-of-range rope edit returns an error.
#[test]
fn rope_out_of_range_returns_error() {
    let content = "short";
    let edit = text_edit(5, 0, 5, 1, "x");
    let mut edits = vec![&edit];
    assert!(apply_edits_to_rope(content, &mut edits).is_err());
}

/// Tests that a single-file edit produces a valid unified diff.
#[test]
fn diff_single_file_single_edit() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "hello world\n").unwrap();
    tmp.flush().unwrap();
    let path = tmp.path().to_str().unwrap();

    let edit = workspace_edit(vec![(uri_from_path(path), vec![text_edit(0, 0, 0, 5, "goodbye")])]);
    let diff = workspace_edit_to_diff(&edit, &passthrough_resolver()).unwrap();

    assert!(diff.contains("--- a/"));
    assert!(diff.contains("+++ b/"));
    assert!(diff.contains("-hello world"));
    assert!(diff.contains("+goodbye world"));
}

/// Tests that multiple edits in one file produce a combined diff.
#[test]
fn diff_multi_edit_single_file() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "aaa bbb ccc\n").unwrap();
    tmp.flush().unwrap();
    let path = tmp.path().to_str().unwrap();

    let edit = workspace_edit(vec![(uri_from_path(path), vec![
        text_edit(0, 0, 0, 3, "xxx"),
        text_edit(0, 8, 0, 11, "zzz"),
    ])]);
    let diff = workspace_edit_to_diff(&edit, &passthrough_resolver()).unwrap();

    assert!(diff.contains("-aaa bbb ccc"));
    assert!(diff.contains("+xxx bbb zzz"));
}

/// Tests that edits across multiple files produce per-file diff sections.
#[test]
fn diff_multiple_files() {
    let mut tmp1 = tempfile::NamedTempFile::new().unwrap();
    write!(tmp1, "file one\n").unwrap();
    tmp1.flush().unwrap();
    let path1 = tmp1.path().to_str().unwrap().to_owned();

    let mut tmp2 = tempfile::NamedTempFile::new().unwrap();
    write!(tmp2, "file two\n").unwrap();
    tmp2.flush().unwrap();
    let path2 = tmp2.path().to_str().unwrap().to_owned();

    let edit = workspace_edit(vec![
        (uri_from_path(&path1), vec![text_edit(0, 5, 0, 8, "ONE")]),
        (uri_from_path(&path2), vec![text_edit(0, 5, 0, 8, "TWO")]),
    ]);
    let diff = workspace_edit_to_diff(&edit, &passthrough_resolver()).unwrap();

    // Both files should appear in the diff.
    assert!(diff.contains("-file one"));
    assert!(diff.contains("+file ONE"));
    assert!(diff.contains("-file two"));
    assert!(diff.contains("+file TWO"));
}

/// Tests that a pure insertion produces a diff with added lines.
#[test]
fn diff_insertion_only() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "line one\nline two\n").unwrap();
    tmp.flush().unwrap();
    let path = tmp.path().to_str().unwrap();

    // Insert a new line between line one and line two.
    let edit = workspace_edit(vec![(uri_from_path(path), vec![text_edit(1, 0, 1, 0, "inserted\n")])]);
    let diff = workspace_edit_to_diff(&edit, &passthrough_resolver()).unwrap();

    assert!(diff.contains("+inserted"));
}

/// Tests that a pure deletion produces a diff with removed lines.
#[test]
fn diff_deletion_only() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "keep\nremove\ntrailing\n").unwrap();
    tmp.flush().unwrap();
    let path = tmp.path().to_str().unwrap();

    // Delete the second line entirely (line 1 through start of line 2).
    let edit = workspace_edit(vec![(uri_from_path(path), vec![text_edit(1, 0, 2, 0, "")])]);
    let diff = workspace_edit_to_diff(&edit, &passthrough_resolver()).unwrap();

    assert!(diff.contains("-remove"));
}

/// Tests that an empty workspace edit produces an empty diff string.
#[test]
fn diff_empty_edit_returns_empty_string() {
    let edit = WorkspaceEdit {
        changes: Some(HashMap::new()),
        document_changes: None,
        change_annotations: None,
    };

    let diff = workspace_edit_to_diff(&edit, &passthrough_resolver()).unwrap();
    assert!(diff.is_empty());
}

/// Tests that replacing text with identical content produces an empty diff.
#[test]
fn diff_no_change_returns_empty_diff() {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "unchanged\n").unwrap();
    tmp.flush().unwrap();
    let path = tmp.path().to_str().unwrap();

    // Replace "unchanged" with "unchanged" — no actual change.
    let edit = workspace_edit(vec![(uri_from_path(path), vec![text_edit(0, 0, 0, 9, "unchanged")])]);
    let diff = workspace_edit_to_diff(&edit, &passthrough_resolver()).unwrap();

    // similar produces empty output when old == new.
    assert!(diff.is_empty());
}
