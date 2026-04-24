use rstest::rstest;

use crate::syntax::fragment::{DecomposedFile, FragmentKind, SymbolKind};

crate::language_tests! {
    ext: "py",
    fixture_module: "syntax/languages/python",
    fixture_file: "basic.py",
    fragment_count: 6,
    fragment_names: [
        "imports",
        "MAX_RETRIES",
        "DEFAULT_NAME",
        "greet",
        "Config",
        "Processor",
    ],
    fragment_kinds: [
        FragmentKind::Imports,
        FragmentKind::Symbol(SymbolKind::Variable),
        FragmentKind::Symbol(SymbolKind::Variable),
        FragmentKind::Symbol(SymbolKind::Function),
        FragmentKind::Symbol(SymbolKind::Class),
        FragmentKind::Symbol(SymbolKind::Class),
    ],
    imports_contain: ["import os", "from pathlib import Path"],
}

/// Verifies that Python class fragments expose the expected child symbols
/// (docstring, decorators, annotated fields, methods).
#[rstest]
#[case::config_class("Config", &["docstring", "decorators", "name", "debug", "validate", "reset"])]
#[case::processor_class("Processor", &["__init__", "run"])]
fn class_children_cases(basic: DecomposedFile, #[case] class_name: &str, #[case] expected: &[&str]) {
    crate::test_support::assert_fragment_children(&basic, class_name, expected);
}
