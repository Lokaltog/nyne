use rstest::{fixture, rstest};

use crate::syntax::fragment::{DecomposedFile, FragmentKind, SymbolKind};
use crate::test_support::{load_fixture, registry};

/// Fixture: decompose the basic.fnl test file into fragments.
#[fixture]
fn basic() -> DecomposedFile {
    let source = load_fixture("syntax/languages/fennel", "basic.fnl");
    let reg = registry();
    let d = reg.get("fnl").unwrap();
    let (result, _tree) = d.decompose(&source, 5);
    result
}

/// Top-level: MAX-RETRIES, greet, process, with-retry, config = 5 fragments.
/// The require forms (lume, utils) should be coalesced into imports.
#[rstest]
fn fragment_count(basic: DecomposedFile) {
    assert_eq!(basic.len(), 5);
}

/// Verifies that fragment names match the expected symbol names in order.
#[rstest]
fn fragment_names(basic: DecomposedFile) {
    let names: Vec<_> = basic.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, &["MAX-RETRIES", "greet", "process", "with-retry", "config"]);
}

/// Verifies that fragment kinds match the expected symbol kinds in order.
#[rstest]
fn fragment_kinds(basic: DecomposedFile) {
    let kinds: Vec<_> = basic.iter().map(|f| &f.kind).collect();
    assert_eq!(kinds, &[
        &FragmentKind::Symbol(SymbolKind::Variable),
        &FragmentKind::Symbol(SymbolKind::Function),
        &FragmentKind::Symbol(SymbolKind::Function),
        &FragmentKind::Symbol(SymbolKind::Macro),
        &FragmentKind::Symbol(SymbolKind::Variable),
    ]);
}

/// Require bindings are excluded from fragments (not meaningful decomposition targets).
#[rstest]
fn require_bindings_excluded(basic: DecomposedFile) {
    let names: Vec<_> = basic.iter().map(|f| f.name.as_str()).collect();
    assert!(!names.contains(&"lume"), "require binding 'lume' should be excluded");
    assert!(!names.contains(&"utils"), "require binding 'utils' should be excluded");
}

/// No nested children — Fennel forms are flat.
#[rstest]
fn no_children(basic: DecomposedFile) {
    for frag in &basic {
        assert!(
            frag.children.is_empty(),
            "fragment '{}' should have no children",
            frag.name
        );
    }
}
