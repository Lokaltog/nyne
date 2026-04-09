use nyne::load_fixture;
use rstest::{fixture, rstest};

use crate::syntax::fragment::{DecomposedFile, FragmentKind, SymbolKind};
use crate::test_support::registry;

/// Load `basic.py` fixture source. Single source of truth for the fixture path.
fn load_basic() -> String { load_fixture!("syntax/languages/python", "basic.py") }

/// Fixture: decompose the basic.py test file into fragments.
#[fixture]
fn basic() -> DecomposedFile {
    let (result, _tree) = registry().get("py").unwrap().decompose(&load_basic(), 5);
    result
}

/// Top-level: imports + 2 assignments + 1 function + 2 classes = 6 fragments.
#[rstest]
fn fragment_count(basic: DecomposedFile) {
    assert_eq!(basic.len(), 6);
}

/// Verifies that fragment names match the expected symbol names in order.
#[rstest]
fn fragment_names(basic: DecomposedFile) {
    let names: Vec<_> = basic.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, &[
        "imports",
        "MAX_RETRIES",
        "DEFAULT_NAME",
        "greet",
        "Config",
        "Processor"
    ]);
}

/// Verifies that fragment kinds match the expected symbol kinds in order.
#[rstest]
fn fragment_kinds(basic: DecomposedFile) {
    let kinds: Vec<_> = basic.iter().map(|f| &f.kind).collect();
    assert_eq!(kinds, &[
        &FragmentKind::Imports,
        &FragmentKind::Symbol(SymbolKind::Variable),
        &FragmentKind::Symbol(SymbolKind::Variable),
        &FragmentKind::Symbol(SymbolKind::Function),
        &FragmentKind::Symbol(SymbolKind::Class),
        &FragmentKind::Symbol(SymbolKind::Class),
    ]);
}

/// Imports are coalesced into a single Imports fragment.
#[rstest]
fn imports_extracted(basic: DecomposedFile) {
    let range = crate::syntax::fragment::find_fragment_of_kind(&basic, &FragmentKind::Imports)
        .expect("imports fragment should be present")
        .byte_range
        .clone();
    let source = load_basic();
    assert!(source[range.clone()].contains("import os"));
    assert!(source[range].contains("from pathlib import Path"));
}

/// Class `Config` has docstring, decorators, field annotations and methods as children.
#[rstest]
fn class_children(basic: DecomposedFile) {
    let config = basic.iter().find(|f| f.name == "Config").unwrap();
    let child_names: Vec<_> = config.children.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(child_names, &[
        "docstring",
        "decorators",
        "name",
        "debug",
        "validate",
        "reset"
    ]);
}

/// Class `Processor` has 2 methods.
#[rstest]
fn processor_children(basic: DecomposedFile) {
    let processor = basic.iter().find(|f| f.name == "Processor").unwrap();
    let child_names: Vec<_> = processor.children.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(child_names, &["__init__", "run"]);
}
