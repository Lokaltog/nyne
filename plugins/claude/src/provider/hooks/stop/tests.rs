use std::collections::BTreeSet;

use rstest::rstest;

use super::*;

/// Builds a test ignore list of file extensions.
fn ignore_list() -> Vec<String> { ["toml", "md", "json"].into_iter().map(String::from).collect() }

/// Verifies extension-based file filtering across various file types.
#[rstest]
#[case::rust_file("src/lib.rs", false)]
#[case::python_file("scripts/run.py", false)]
#[case::toml_config("Cargo.toml", true)]
#[case::markdown_doc("README.md", true)]
#[case::json_file("package.json", true)]
#[case::uppercase_ext("NOTES.MD", true)]
#[case::no_extension("Makefile", false)]
#[case::nested_path("src/config/mod.rs", false)]
#[case::dotfile(".gitignore", false)]
fn is_ignored_extension_cases(#[case] path: &str, #[case] expected: bool) {
    assert_eq!(is_ignored_extension(path, &ignore_list()), expected);
}

/// Build a minimal transcript line containing an assistant Edit/Write `tool_use`.
fn tool_use_line(tool: &str, file_path: &str) -> String {
    serde_json::json!({
        "type": "assistant",
        "message": {
            "content": [{
                "type": "tool_use",
                "name": tool,
                "input": { "file_path": file_path }
            }]
        }
    })
    .to_string()
}

/// Verifies extraction of changed file paths from transcript lines.
#[rstest]
#[case::edit_rs("Edit", "/project/src/lib.rs", &[], 1)]
#[case::write_rs("Write", "/project/src/main.rs", &[], 1)]
#[case::edit_toml_ignored("Edit", "/project/Cargo.toml", &["toml"], 0)]
#[case::edit_md_ignored("Edit", "/project/README.md", &["md"], 0)]
#[case::non_edit_tool("Read", "/project/src/lib.rs", &[], 0)]
fn extract_changed_paths_cases(
    #[case] tool: &str,
    #[case] path: &str,
    #[case] ignored: &[&str],
    #[case] expected_count: usize,
) {
    let line = tool_use_line(tool, path);
    let ignore_exts: Vec<String> = ignored.iter().map(|s| (*s).to_owned()).collect();
    let mut out = BTreeSet::new();
    extract_changed_paths(&line, "/project/", &ignore_exts, &mut out);
    assert_eq!(out.len(), expected_count);
}

/// Tests that extracted paths have the project root prefix stripped.
#[test]
fn extract_strips_root_prefix() {
    let line = tool_use_line("Edit", "/project/src/lib.rs");
    let mut out = BTreeSet::new();
    extract_changed_paths(&line, "/project/", &[], &mut out);
    assert_eq!(out.into_iter().next().unwrap(), "src/lib.rs");
}

/// Tests that duplicate file paths across tool uses are deduplicated.
#[test]
fn extract_deduplicates() {
    let line = serde_json::json!({
        "type": "assistant",
        "message": {
            "content": [
                { "type": "tool_use", "name": "Edit", "input": { "file_path": "/p/a.rs" } },
                { "type": "tool_use", "name": "Write", "input": { "file_path": "/p/a.rs" } },
            ]
        }
    })
    .to_string();

    let mut out = BTreeSet::new();
    extract_changed_paths(&line, "/p/", &[], &mut out);
    assert_eq!(out.len(), 1);
}
