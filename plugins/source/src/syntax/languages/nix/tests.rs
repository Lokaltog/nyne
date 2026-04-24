use rstest::rstest;

use crate::syntax::fragment::{DecomposedFile, FragmentKind, SymbolKind};

crate::language_tests! {
    ext: "nix",
    fixture_module: "syntax/languages/nix",
    fixture_file: "basic.nix",
    fragment_count: 5,
    fragment_names: ["name", "version", "buildInputs", "meta", "shellHook"],
    fragment_kinds: [
        FragmentKind::Symbol(SymbolKind::Variable),
        FragmentKind::Symbol(SymbolKind::Variable),
        FragmentKind::Symbol(SymbolKind::Variable),
        FragmentKind::Symbol(SymbolKind::Module),
        FragmentKind::Symbol(SymbolKind::Variable),
    ],
}

/// `meta` has nested attrset children (one level of decomposition).
#[rstest]
fn meta_children(basic: DecomposedFile) {
    crate::test_support::assert_fragment_children(&basic, "meta", &["description", "license", "maintainers"]);
}

/// No imports in Nix (currently).
#[rstest]
fn no_imports(basic: DecomposedFile) {
    use crate::syntax::fragment::find_fragment_of_kind;
    assert!(find_fragment_of_kind(&basic, &FragmentKind::Imports).is_none());
}
