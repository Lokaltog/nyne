use rstest::{fixture, rstest};

use crate::syntax::fragment::{DEFAULT_MAX_DEPTH, DecomposedFile, FragmentKind, SymbolKind};
use crate::test_support::{load_fixture, registry};

/// Fixture: decompose the basic.ts test file into fragments.
#[fixture]
fn basic() -> DecomposedFile {
    let source = load_fixture("syntax/languages/typescript", "basic.ts");
    let reg = registry();
    let d = reg.get("ts").unwrap();
    let (result, _tree) = d.decompose(&source, DEFAULT_MAX_DEPTH);
    result
}

/// Top-level: imports + const + 2 fns + interface + class + enum + type alias = 8 fragments.
#[rstest]
fn fragment_count(basic: DecomposedFile) {
    assert_eq!(basic.len(), 8);
}

/// Verifies that fragment names match the expected symbol names in order.
#[rstest]
fn fragment_names(basic: DecomposedFile) {
    let names: Vec<_> = basic.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, &[
        "imports",
        "MAX_RETRIES",
        "greet",
        "helper",
        "Processor",
        "AppConfig",
        "Status",
        "Result"
    ]);
}

/// Verifies that fragment kinds match the expected symbol kinds in order.
#[rstest]
fn fragment_kinds(basic: DecomposedFile) {
    let kinds: Vec<_> = basic.iter().map(|f| &f.kind).collect();
    assert_eq!(kinds, &[
        &FragmentKind::Imports,
        &FragmentKind::Symbol(SymbolKind::Variable),
        &FragmentKind::Symbol(SymbolKind::Function),
        &FragmentKind::Symbol(SymbolKind::Function),
        &FragmentKind::Symbol(SymbolKind::Interface),
        &FragmentKind::Symbol(SymbolKind::Class),
        &FragmentKind::Symbol(SymbolKind::Enum),
        &FragmentKind::Symbol(SymbolKind::TypeAlias),
    ]);
}

/// Imports are coalesced into a single Imports fragment.
#[rstest]
fn imports_extracted(basic: DecomposedFile) {
    let source = load_fixture("syntax/languages/typescript", "basic.ts");
    let imports_frag = crate::syntax::fragment::find_fragment_of_kind(&basic, &FragmentKind::Imports)
        .expect("imports fragment should be present");
    let imports_text = &source[imports_frag.byte_range.clone()];
    assert!(imports_text.contains("import { readFile }"));
    assert!(imports_text.contains("import type { Config }"));
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
