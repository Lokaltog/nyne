use rstest::rstest;

use super::*;

/// Qualified paths return the last segment as a caller search.
#[rstest]
#[case::simple_qualified("Foo::bar", "bar")]
#[case::deeply_qualified("std::collections::HashMap", "HashMap")]
#[case::trailing_paren("Foo::new(", "new")]
fn extract_symbol_qualified_path(#[case] pattern: &str, #[case] expected_sym: &str) {
    let (kind, sym) = extract_symbol_from_grep(pattern).unwrap();
    assert_eq!(kind, "callers");
    assert_eq!(sym, expected_sym);
}

/// Method call and `fn` declaration patterns return callers.
#[rstest]
#[case::dot_method("\\.process(", "process")]
#[case::fn_decl("fn handle_request", "handle_request")]
fn extract_symbol_method_patterns(#[case] pattern: &str, #[case] expected_sym: &str) {
    let (kind, sym) = extract_symbol_from_grep(pattern).unwrap();
    assert_eq!(kind, "callers");
    assert_eq!(sym, expected_sym);
}

/// Bare function call patterns (with parenthesis) return callers.
#[rstest]
#[case::escaped_paren("process\\(", "process")]
#[case::literal_paren("process(arg", "process")]
fn extract_symbol_function_call(#[case] pattern: &str, #[case] expected_sym: &str) {
    let (kind, sym) = extract_symbol_from_grep(pattern).unwrap();
    assert_eq!(kind, "callers");
    assert_eq!(sym, expected_sym);
}

/// `PascalCase` identifiers return references.
#[rstest]
#[case::simple_type("HashMap", "HashMap")]
#[case::with_underscore("My_Type", "My_Type")]
#[case::single_upper("A", "A")]
fn extract_symbol_type_name(#[case] pattern: &str, #[case] expected_sym: &str) {
    let (kind, sym) = extract_symbol_from_grep(pattern).unwrap();
    assert_eq!(kind, "references");
    assert_eq!(sym, expected_sym);
}

/// Import statement patterns return imports with empty symbol.
#[rstest]
#[case::rust_use("use serde")]
#[case::js_import("import React")]
#[case::python_from("from pathlib import Path")]
fn extract_symbol_import(#[case] pattern: &str) {
    let (kind, sym) = extract_symbol_from_grep(pattern).unwrap();
    assert_eq!(kind, "imports");
    assert!(sym.is_empty());
}

/// Patterns that don't match any heuristic return None.
#[rstest]
#[case::plain_lowercase("hello world")]
#[case::number("42")]
#[case::empty("")]
#[case::regex_only(".*")]
#[case::lowercase_no_paren("some_var")]
fn extract_symbol_returns_none(#[case] pattern: &str) {
    assert!(extract_symbol_from_grep(pattern).is_none());
}

/// `extract_first_identifier` skips regex metacharacters and the `fn` keyword.
#[rstest]
#[case::leading_backslash_dot("\\.process(", "process")]
#[case::fn_prefix("fn handle", "handle")]
#[case::bare_word("hello", "hello")]
#[case::leading_caret("^start", "start")]
fn extract_first_identifier_cases(#[case] pattern: &str, #[case] expected: &str) {
    assert_eq!(extract_first_identifier(pattern).unwrap(), expected);
}

/// `extract_first_identifier` returns None for patterns with no identifiers.
#[rstest]
#[case::empty("")]
#[case::only_symbols("...*+?")]
fn extract_first_identifier_none(#[case] pattern: &str) {
    assert!(extract_first_identifier(pattern).is_none());
}
