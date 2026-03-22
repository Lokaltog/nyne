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
