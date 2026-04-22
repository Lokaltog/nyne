use rstest::rstest;

use crate::syntax::fragment::{DecomposedFile, FragmentKind, SymbolKind};

crate::language_tests! {
    ext: "toml",
    fixture_module: "syntax/languages/toml",
    fixture_file: "basic.toml",
    fragment_count: 6,
    fragment_names: ["preamble", "package", "dependencies", "dev-dependencies", "bin", "bin"],
    fragment_kinds: [
        FragmentKind::Preamble,
        FragmentKind::Symbol(SymbolKind::Module),
        FragmentKind::Symbol(SymbolKind::Module),
        FragmentKind::Symbol(SymbolKind::Module),
        FragmentKind::Symbol(SymbolKind::Module),
        FragmentKind::Symbol(SymbolKind::Module),
    ],
}

/// First fragment is a preamble containing bare top-level key-value pairs.
#[rstest]
fn preamble_is_first(basic: DecomposedFile) {
    let first = &basic[0];
    assert_eq!(first.name, "preamble");
    assert_eq!(first.kind, FragmentKind::Preamble);
}

/// Table sections are `Module` symbols (opaque, no children).
#[rstest]
fn table_sections_are_opaque(basic: DecomposedFile) {
    for frag in &basic[1..] {
        assert_eq!(
            frag.kind,
            FragmentKind::Symbol(SymbolKind::Module),
            "fragment '{}' should be Module",
            frag.name
        );
        assert!(
            frag.children.is_empty(),
            "fragment '{}' should have no children",
            frag.name
        );
    }
}

/// No imports in TOML.
#[rstest]
fn no_imports(basic: DecomposedFile) {
    use crate::syntax::fragment::find_fragment_of_kind;
    assert!(find_fragment_of_kind(&basic, &FragmentKind::Imports).is_none());
}
