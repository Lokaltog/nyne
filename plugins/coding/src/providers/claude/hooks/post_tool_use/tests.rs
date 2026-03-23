use rstest::rstest;
use serde_json::json;

use super::*;

/// Build a `HookInput` with `tool_input` containing the given file path.
fn file_tool_input(file_path: &str) -> HookInput {
    serde_json::from_value(json!({ "tool_input": { "file_path": file_path } })).unwrap()
}

/// Build a `HookInput` with a Bash command (no file_path field).
fn bash_input() -> HookInput { serde_json::from_value(json!({ "tool_input": { "command": "ls" } })).unwrap() }

#[rstest]
#[case::edit_raw("/code/src/lib.rs", "Edit", Some("src/lib.rs"))]
#[case::write_raw("/code/src/main.rs", "Write", Some("src/main.rs"))]
#[case::edit_vfs("/code/src/lib.rs@/symbols/Foo@/body.rs", "Edit", Some("src/lib.rs"))]
#[case::write_vfs("/code/src/main.rs@/symbols/Bar.rs", "Write", Some("src/main.rs"))]
#[case::edit_nested_vfs("/code/src/fuse/attrs.rs@/symbols/at-line/127", "Edit", Some("src/fuse/attrs.rs"))]
#[case::bash_ignored("/code/src/lib.rs", "Bash", None)]
#[case::read_ignored("/code/src/lib.rs", "Read", None)]
#[case::outside_root("/other/project/lib.rs", "Edit", None)]
#[case::outside_root_vfs("/other/project/lib.rs@/symbols/Foo.rs", "Edit", None)]
fn source_rel_path_cases(#[case] file_path: &str, #[case] tool_name: &str, #[case] expected: Option<&str>) {
    let input = match tool_name {
        "Edit" | "Write" => file_tool_input(file_path),
        _ => bash_input(),
    };
    assert_eq!(source_rel_path(&input, tool_name, "/code/").as_deref(), expected);
}
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

fn edit_input_replace_all(old_string: &str, new_string: &str) -> HookInput {
    serde_json::from_value(json!({
        "tool_name": "Edit",
        "tool_input": {
            "file_path": "/code/src/lib.rs",
            "old_string": old_string,
            "new_string": new_string,
            "replace_all": true,
        }
    }))
    .unwrap()
}

fn make_decomposed(source: &str) -> DecomposedSource {
    let registry = crate::syntax::SyntaxRegistry::global();
    DecomposedSource {
        source: source.to_owned(),
        decomposed: Default::default(),
        decomposer: Arc::clone(registry.get("rs").unwrap()),
        tree: None,
    }
}

#[test]
fn changed_line_range_single_line_edit() {
    let input = edit_input("old_val", "new_val");
    let source = "fn foo() {\n    let x = new_val;\n}\n";
    let decomposed = make_decomposed(source);
    let range = changed_line_range(&input, "Edit", &decomposed);
    // "new_val" is on line 2 (1-based). Range should include it.
    assert!(range.is_some());
    let r = range.unwrap();
    assert!(r.start <= 2, "start {r:?} should include line 2");
    assert!(r.end > 2, "end {r:?} should include line 2");
}

#[test]
fn changed_line_range_multiline_edit() {
    let input = edit_input("old", "line_a\n    line_b\n    line_c");
    let source = "fn foo() {\n    line_a\n    line_b\n    line_c\n}\n";
    let decomposed = make_decomposed(source);
    let range = changed_line_range(&input, "Edit", &decomposed);
    assert!(range.is_some());
    let r = range.unwrap();
    // Lines 2-4 (1-based) contain the new content.
    assert!(r.start <= 2);
    assert!(r.end >= 4);
}

#[test]
fn changed_line_range_none_for_write() {
    let input = edit_input("old", "new");
    let decomposed = make_decomposed("new\n");
    assert!(changed_line_range(&input, "Write", &decomposed).is_none());
}

#[test]
fn changed_line_range_none_for_empty_new_string() {
    let input = edit_input("deleted", "");
    let decomposed = make_decomposed("fn foo() {}\n");
    assert!(changed_line_range(&input, "Edit", &decomposed).is_none());
}

#[test]
fn changed_line_range_none_for_replace_all() {
    let input = edit_input_replace_all("old", "new");
    let decomposed = make_decomposed("new and new\n");
    assert!(changed_line_range(&input, "Edit", &decomposed).is_none());
}

#[test]
fn changed_line_range_none_for_ambiguous_match() {
    let input = edit_input("x", "val");
    // "val" appears twice — ambiguous.
    let decomposed = make_decomposed("let a = val;\nlet b = val;\n");
    assert!(changed_line_range(&input, "Edit", &decomposed).is_none());
}

#[test]
fn filter_hints_passes_all_when_no_range() {
    let hints = vec![
        HintView {
            rule_id: "r1",
            severity: "info",
            message: "a".into(),
            line_start: 1,
            line_end: 1,
            suggestions: vec![],
        },
        HintView {
            rule_id: "r2",
            severity: "info",
            message: "b".into(),
            line_start: 50,
            line_end: 50,
            suggestions: vec![],
        },
    ];
    let result = filter_hints(hints.clone(), None);
    assert_eq!(result.len(), 2);
}

#[test]
fn filter_hints_narrows_to_range() {
    let hints = vec![
        HintView {
            rule_id: "r1",
            severity: "info",
            message: "before".into(),
            line_start: 1,
            line_end: 1,
            suggestions: vec![],
        },
        HintView {
            rule_id: "r2",
            severity: "info",
            message: "inside".into(),
            line_start: 10,
            line_end: 12,
            suggestions: vec![],
        },
        HintView {
            rule_id: "r3",
            severity: "info",
            message: "after".into(),
            line_start: 50,
            line_end: 50,
            suggestions: vec![],
        },
    ];
    let range = 8..15;
    let result = filter_hints(hints, Some(&range));
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].rule_id, "r2");
}

#[test]
fn filter_hints_includes_overlapping() {
    let hints = vec![HintView {
        rule_id: "r1",
        severity: "info",
        message: "spans into range".into(),
        line_start: 5,
        line_end: 12,
        suggestions: vec![],
    }];
    let range = 10..20;
    let result = filter_hints(hints, Some(&range));
    assert_eq!(result.len(), 1, "hint overlapping range start should be included");
}

#[test]
fn filter_diagnostics_passes_all_when_no_range() {
    let diags = vec![
        DiagnosticRow {
            line: 1,
            col: 1,
            severity: "error",
            code: String::new(),
            source: String::new(),
            message: "a".into(),
        },
        DiagnosticRow {
            line: 99,
            col: 1,
            severity: "error",
            code: String::new(),
            source: String::new(),
            message: "b".into(),
        },
    ];
    let result = filter_diagnostics(diags, None);
    assert_eq!(result.len(), 2);
}

#[test]
fn filter_diagnostics_narrows_to_range() {
    let diags = vec![
        DiagnosticRow {
            line: 1,
            col: 1,
            severity: "error",
            code: String::new(),
            source: String::new(),
            message: "before".into(),
        },
        DiagnosticRow {
            line: 10,
            col: 5,
            severity: "error",
            code: String::new(),
            source: String::new(),
            message: "inside".into(),
        },
        DiagnosticRow {
            line: 50,
            col: 1,
            severity: "error",
            code: String::new(),
            source: String::new(),
            message: "after".into(),
        },
    ];
    let range = 8..15;
    let result = filter_diagnostics(diags, Some(&range));
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].message, "inside");
}
