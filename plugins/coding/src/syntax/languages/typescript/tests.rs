use rstest::{fixture, rstest};

use crate::syntax::fragment::{DEFAULT_MAX_DEPTH, DecomposedFile, FragmentKind, SymbolKind};
use crate::test_support::{load_fixture, registry};

#[fixture]
fn basic() -> DecomposedFile {
    let source = load_fixture("syntax/languages/typescript", "basic.ts");
    let reg = registry();
    let d = reg.get("ts").unwrap();
    let (result, _tree) = d.decompose(&source, DEFAULT_MAX_DEPTH);
    result
}

/// Top-level: const, 2 fns, interface, class, enum, type alias = 7 fragments.
#[rstest]
fn fragment_count(basic: DecomposedFile) {
    assert_eq!(basic.fragments.len(), 7);
}

#[rstest]
fn fragment_names(basic: DecomposedFile) {
    let names: Vec<_> = basic.fragments.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, &[
        "MAX_RETRIES",
        "greet",
        "helper",
        "Processor",
        "AppConfig",
        "Status",
        "Result"
    ]);
}

#[rstest]
fn fragment_kinds(basic: DecomposedFile) {
    let kinds: Vec<_> = basic.fragments.iter().map(|f| &f.kind).collect();
    assert_eq!(kinds, &[
        &FragmentKind::Symbol(SymbolKind::Variable),
        &FragmentKind::Symbol(SymbolKind::Function),
        &FragmentKind::Symbol(SymbolKind::Function),
        &FragmentKind::Symbol(SymbolKind::Interface),
        &FragmentKind::Symbol(SymbolKind::Class),
        &FragmentKind::Symbol(SymbolKind::Enum),
        &FragmentKind::Symbol(SymbolKind::TypeAlias),
    ]);
}

/// Imports are coalesced into a single ImportSpan.
#[rstest]
fn imports_extracted(basic: DecomposedFile) {
    let imports = basic.imports.as_ref().expect("imports should be present");
    assert!(imports.content.contains("import { readFile }"));
    assert!(imports.content.contains("import type { Config }"));
}

/// Class `AppConfig` has 3 method children: constructor, validate, reset.
#[rstest]
fn class_children(basic: DecomposedFile) {
    let config = basic.fragments.iter().find(|f| f.name == "AppConfig").unwrap();
    let child_names: Vec<_> = config.children.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(child_names, &["constructor", "validate", "reset"]);
}

/// Interface has no children (method signatures are part of the body).
#[rstest]
fn interface_no_children(basic: DecomposedFile) {
    let iface = basic.fragments.iter().find(|f| f.name == "Processor").unwrap();
    assert!(iface.children.is_empty());
}
