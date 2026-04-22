use rstest::rstest;

use crate::syntax::fragment::{DecomposedFile, FragmentKind, SymbolKind};

crate::language_tests! {
    ext: "fnl",
    fixture_module: "syntax/languages/fennel",
    fixture_file: "basic.fnl",
    fragment_count: 5,
    fragment_names: ["MAX-RETRIES", "greet", "process", "with-retry", "config"],
    fragment_kinds: [
        FragmentKind::Symbol(SymbolKind::Variable),
        FragmentKind::Symbol(SymbolKind::Function),
        FragmentKind::Symbol(SymbolKind::Function),
        FragmentKind::Symbol(SymbolKind::Macro),
        FragmentKind::Symbol(SymbolKind::Variable),
    ],
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
