use rstest::rstest;

use crate::syntax::fragment::{DecomposedFile, FragmentKind, SymbolKind};

crate::language_tests! {
    ext: "rs",
    fixture_module: "syntax/languages/rust",
    fixture_file: "basic.rs",
    fragment_count: 9,
    fragment_names: [
        "imports",
        "MAX_SIZE",
        "process",
        "helper",
        "Config",
        "Status",
        "Processor",
        "Processor_for_Config",
        "Config",
    ],
    fragment_kinds: [
        FragmentKind::Imports,
        FragmentKind::Symbol(SymbolKind::Const),
        FragmentKind::Symbol(SymbolKind::Function),
        FragmentKind::Symbol(SymbolKind::Function),
        FragmentKind::Symbol(SymbolKind::Struct),
        FragmentKind::Symbol(SymbolKind::Enum),
        FragmentKind::Symbol(SymbolKind::Trait),
        FragmentKind::Symbol(SymbolKind::Impl),
        FragmentKind::Symbol(SymbolKind::Impl),
    ],
    imports_contain: ["use std::collections::HashMap;", "use std::io;"],
}

/// Verifies which Rust fragments expose child fragments:
/// * traits don't decompose method signatures (only definitions with bodies),
/// * trait impls expose their methods,
/// * inherent impls expose their methods (kind filter disambiguates struct vs impl).
#[rstest]
#[case::trait_no_children("Processor", None, &[])]
#[case::trait_impl_children("Processor_for_Config", None, &["run", "reset"])]
#[case::inherent_impl_children(
    "Config",
    Some(FragmentKind::Symbol(SymbolKind::Impl)),
    &["new"],
)]
fn fragment_children(
    basic: DecomposedFile,
    #[case] name: &str,
    #[case] kind: Option<FragmentKind>,
    #[case] expected: &[&str],
) {
    let fragment = basic
        .iter()
        .filter(|f| f.name == name)
        .find(|f| kind.as_ref().is_none_or(|k| &f.kind == k))
        .unwrap_or_else(|| panic!("no matching fragment for {name:?}"));
    let child_names: Vec<_> = fragment.children.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(child_names, expected);
}
