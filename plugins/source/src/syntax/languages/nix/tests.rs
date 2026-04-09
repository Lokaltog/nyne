use nyne::load_fixture;
use rstest::{fixture, rstest};

use crate::syntax::fragment::{DecomposedFile, FragmentKind, SymbolKind};
use crate::test_support::registry;

/// Fixture: decompose the basic.nix test file into fragments.
#[fixture]
fn basic() -> DecomposedFile {
    let (result, _tree) = registry()
        .get("nix")
        .unwrap()
        .decompose(&load_fixture!("syntax/languages/nix", "basic.nix"), 5);
    result
}

/// Top-level: 5 bindings (name, version, buildInputs, meta, shellHook).
#[rstest]
fn fragment_count(basic: DecomposedFile) {
    assert_eq!(basic.len(), 5);
}

/// Verifies that fragment names match the expected binding names in order.
#[rstest]
fn fragment_names(basic: DecomposedFile) {
    let names: Vec<_> = basic.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, &["name", "version", "buildInputs", "meta", "shellHook"]);
}

/// Simple bindings are Variables, attrset binding is Module.
#[rstest]
fn fragment_kinds(basic: DecomposedFile) {
    let kinds: Vec<_> = basic.iter().map(|f| &f.kind).collect();
    assert_eq!(kinds, &[
        &FragmentKind::Symbol(SymbolKind::Variable),
        &FragmentKind::Symbol(SymbolKind::Variable),
        &FragmentKind::Symbol(SymbolKind::Variable),
        &FragmentKind::Symbol(SymbolKind::Module),
        &FragmentKind::Symbol(SymbolKind::Variable),
    ]);
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
