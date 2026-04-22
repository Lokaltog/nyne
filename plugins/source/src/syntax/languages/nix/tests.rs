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
    let meta = basic.iter().find(|f| f.name == "meta").unwrap();
    let child_names: Vec<_> = meta.children.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(child_names, &["description", "license", "maintainers"]);
}

/// No imports in Nix (currently).
#[rstest]
fn no_imports(basic: DecomposedFile) {
    use crate::syntax::fragment::find_fragment_of_kind;
    assert!(find_fragment_of_kind(&basic, &FragmentKind::Imports).is_none());
}
