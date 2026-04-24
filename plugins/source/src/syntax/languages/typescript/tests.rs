use rstest::rstest;

use crate::syntax::fragment::{DecomposedFile, FragmentKind, SymbolKind};

crate::language_tests! {
    ext: "ts",
    fixture_module: "syntax/languages/typescript",
    fixture_file: "basic.ts",
    fragment_count: 8,
    fragment_names: [
        "imports",
        "MAX_RETRIES",
        "greet",
        "helper",
        "Processor",
        "AppConfig",
        "Status",
        "Result",
    ],
    fragment_kinds: [
        FragmentKind::Imports,
        FragmentKind::Symbol(SymbolKind::Variable),
        FragmentKind::Symbol(SymbolKind::Function),
        FragmentKind::Symbol(SymbolKind::Function),
        FragmentKind::Symbol(SymbolKind::Interface),
        FragmentKind::Symbol(SymbolKind::Class),
        FragmentKind::Symbol(SymbolKind::Enum),
        FragmentKind::Symbol(SymbolKind::TypeAlias),
    ],
    imports_contain: ["import { readFile }", "import type { Config }"],
}

/// Verifies TypeScript fragment children: classes expose methods;
/// interfaces are opaque (method signatures are part of the body).
#[rstest]
#[case::class_has_methods("AppConfig", &["constructor", "validate", "reset"])]
#[case::interface_no_children("Processor", &[])]
fn fragment_children(basic: DecomposedFile, #[case] name: &str, #[case] expected: &[&str]) {
    crate::test_support::assert_fragment_children(&basic, name, expected);
}
