use rstest::{fixture, rstest};

use crate::syntax::fragment::{DEFAULT_MAX_DEPTH, DecomposedFile, FragmentKind, SymbolKind};
use crate::test_support::{load_fixture, registry};

#[fixture]
fn basic() -> DecomposedFile {
    let source = load_fixture("syntax/languages/fennel", "basic.fnl");
    let reg = registry();
    let d = reg.get("fnl").unwrap();
    let (result, _tree) = d.decompose(&source, DEFAULT_MAX_DEPTH);
    result
}

/// Top-level: MAX-RETRIES, greet, process, with-retry, config = 5 fragments.
/// The require forms (lume, utils) should be coalesced into imports.
#[rstest]
fn fragment_count(basic: DecomposedFile) {
    assert_eq!(basic.fragments.len(), 5);
}

#[rstest]
fn fragment_names(basic: DecomposedFile) {
    let names: Vec<_> = basic.fragments.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, &["MAX-RETRIES", "greet", "process", "with-retry", "config"]);
}

#[rstest]
fn fragment_kinds(basic: DecomposedFile) {
    let kinds: Vec<_> = basic.fragments.iter().map(|f| &f.kind).collect();
    assert_eq!(kinds, &[
        &FragmentKind::Symbol(SymbolKind::Variable),
        &FragmentKind::Symbol(SymbolKind::Function),
        &FragmentKind::Symbol(SymbolKind::Function),
        &FragmentKind::Symbol(SymbolKind::Macro),
        &FragmentKind::Symbol(SymbolKind::Variable),
    ]);
}

/// Require forms are coalesced into imports.
#[rstest]
fn imports_extracted(basic: DecomposedFile) {
    let imports = basic.imports.as_ref().expect("imports should be present");
    assert!(imports.content.contains("(require :lume)"));
    assert!(imports.content.contains("(require :utils)"));
}

/// No nested children — Fennel forms are flat.
#[rstest]
fn no_children(basic: DecomposedFile) {
    for frag in &basic.fragments {
        assert!(frag.children.is_empty(), "fragment '{}' should have no children", frag.name);
    }
}
