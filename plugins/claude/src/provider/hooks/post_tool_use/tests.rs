use rstest::rstest;
use serde_json::json;
#[cfg(feature = "analysis")]
use {super::analysis::filter_hints, nyne_analysis::HintView};

use super::*;

// TODO: source_rel_path now requires a Chain for VFS path resolution.
// VFS cases (edit_vfs, write_vfs, edit_nested_vfs, outside_root_vfs) need
// integration tests with a real middleware chain. Non-VFS cases are covered
// by the extract_command_name and extract_rel_paths tests below.

/// Builds a `HookInput` for an Edit tool with old/new string replacement.
fn edit_input(old_string: &str, new_string: &str) -> HookInput {
    serde_json::from_value(json!({
        "tool_name": "Edit",
        "tool_input": {
            "file_path": "/code/src/lib.rs",
            "old_string": old_string,
            "new_string": new_string,
        }
    }))
    .unwrap()
}

/// Builds a `DecomposedSource` from raw Rust source for testing.
fn make_decomposed(source: &str) -> DecomposedSource {
    DecomposedSource {
        source: source.to_owned(),
        decomposed: Default::default(),
        decomposer: Arc::clone(nyne_source::SyntaxRegistry::global().get("rs").unwrap()),
        tree: None,
    }
}

/// Builds a `HintView` fixture with the given rule ID and line range.
#[cfg(feature = "analysis")]
fn hint(rule_id: &'static str, line_start: usize, line_end: usize) -> HintView {
    HintView {
        rule_id,
        severity: "info",
        message: String::new(),
        line_start,
        line_end,
        suggestions: &[],
    }
}

/// Builds a `DiagnosticRow` fixture with the given line and message.
fn diag(line: u32, message: &str) -> DiagnosticRow {
    DiagnosticRow {
        line,
        col: 1,
        severity: "error",
        code: String::new(),
        source: String::new(),
        message: message.into(),
    }
}

/// Verifies that `changed_line_range` returns a range covering the edited lines.
#[rstest]
#[case::single_line("old_val", "new_val", "fn foo() {\n    let x = new_val;\n}\n", 2, 2)]
#[case::multiline(
    "old",
    "line_a\n    line_b\n    line_c",
    "fn foo() {\n    line_a\n    line_b\n    line_c\n}\n",
    2,
    4
)]
fn changed_line_range_includes_edit(
    #[case] old: &str,
    #[case] new: &str,
    #[case] source: &str,
    #[case] min_line: usize,
    #[case] max_line: usize,
) {
    let input = edit_input(old, new);
    let edit = input.tool_input_as::<EditToolInput>();
    let decomposed = make_decomposed(source);
    let range = changed_line_range(edit.as_ref(), &decomposed).expect("should return Some");
    assert!(
        range.start <= min_line,
        "start {range:?} should include line {min_line}"
    );
    assert!(range.end > max_line, "end {range:?} should include line {max_line}");
}

/// Verifies that `changed_line_range` returns None for ambiguous or non-Edit inputs.
#[rstest]
#[case::write(None, "new\n")]
#[case::empty_new(Some(("deleted", "", false)), "fn foo() {}\n")]
#[case::replace_all(Some(("old", "new", true)), "new and new\n")]
#[case::ambiguous(Some(("x", "val", false)), "let a = val;\nlet b = val;\n")]
fn changed_line_range_returns_none(#[case] edit_args: Option<(&str, &str, bool)>, #[case] source: &str) {
    let input: Option<HookInput> = edit_args.map(|(old, new, repl)| {
        if repl {
            serde_json::from_value(json!({
                "tool_name": "Edit",
                "tool_input": { "file_path": "/code/src/lib.rs", "old_string": old, "new_string": new, "replace_all": true }
            }))
            .unwrap()
        } else {
            edit_input(old, new)
        }
    });
    let edit = input
        .as_ref()
        .and_then(crate::provider::hook_schema::HookInput::tool_input_as::<EditToolInput>);
    let decomposed = make_decomposed(source);
    assert!(changed_line_range(edit.as_ref(), &decomposed).is_none());
}

/// Verifies that hints are filtered to the given line range.
#[cfg(feature = "analysis")]
#[rstest]
#[case::no_range_passes_all(
    vec![hint("r1", 1, 1), hint("r2", 50, 50)],
    None,
    2
)]
#[case::narrows_to_range(
    vec![hint("r1", 1, 1), hint("r2", 10, 12), hint("r3", 50, 50)],
    Some(8..15),
    1
)]
#[case::includes_overlapping(
    vec![hint("r1", 5, 12)],
    Some(10..20),
    1
)]
fn filter_hints_cases(#[case] hints: Vec<HintView>, #[case] range: Option<Range<usize>>, #[case] expected: usize) {
    let result = filter_hints(hints, range.as_ref());
    assert_eq!(result.len(), expected);
}

/// Verifies that diagnostics are filtered to the given line range.
#[rstest]
#[case::no_range_passes_all(vec![diag(1, "a"), diag(99, "b")], None, 2)]
#[case::narrows_to_range(vec![diag(1, "before"), diag(10, "inside"), diag(50, "after")], Some(8..15), 1)]
fn filter_diagnostics_cases(
    #[case] diagnostics: Vec<DiagnosticRow>,
    #[case] range: Option<Range<usize>>,
    #[case] expected: usize,
) {
    let result = filter_diagnostics(diagnostics, range.as_ref());
    assert_eq!(result.len(), expected);
}
