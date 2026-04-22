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
}

/// Imports are coalesced into a single Imports fragment.
#[rstest]
fn imports_extracted(basic: DecomposedFile) {
    let range = crate::syntax::fragment::find_fragment_of_kind(&basic, &FragmentKind::Imports)
        .expect("imports fragment should be present")
        .span
        .byte_range
        .clone();
    let source = load_basic();
    assert!(source[range.clone()].contains("import { readFile }"));
    assert!(source[range].contains("import type { Config }"));
}

/// Class `AppConfig` has 3 method children: constructor, validate, reset.
#[rstest]
fn class_children(basic: DecomposedFile) {
    let config = basic.iter().find(|f| f.name == "AppConfig").unwrap();
    let child_names: Vec<_> = config.children.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(child_names, &["constructor", "validate", "reset"]);
}

/// Interface has no children (method signatures are part of the body).
#[rstest]
fn interface_no_children(basic: DecomposedFile) {
    let iface = basic.iter().find(|f| f.name == "Processor").unwrap();
    assert!(iface.children.is_empty());
}
