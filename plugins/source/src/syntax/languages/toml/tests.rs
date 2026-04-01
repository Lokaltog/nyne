use rstest::{fixture, rstest};

use crate::syntax::fragment::{DecomposedFile, FragmentKind, SymbolKind};
use crate::test_support::{load_fixture, registry};

/// Fixture: decompose the basic.toml test file into fragments.
#[fixture]
fn basic() -> DecomposedFile {
    let source = load_fixture("syntax/languages/toml", "basic.toml");
    let reg = registry();
    let d = reg.get("toml").unwrap();
    let (result, _tree) = d.decompose(&source, 5);
    result
}

/// Top-level: preamble (bare keys) + [package] + [dependencies] +
/// [dev-dependencies] + [[bin]] + [[bin]] = 6 fragments.
#[rstest]
fn fragment_count(basic: DecomposedFile) {
    assert_eq!(basic.len(), 6);
}

/// Verifies that fragment names match the expected table names in order.
#[rstest]
fn fragment_names(basic: DecomposedFile) {
    let names: Vec<_> = basic.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, &[
        "preamble",
        "package",
        "dependencies",
        "dev-dependencies",
        "bin",
        "bin"
    ]);
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
