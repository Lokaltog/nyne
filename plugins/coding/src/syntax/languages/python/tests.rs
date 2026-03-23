use rstest::{fixture, rstest};

use crate::syntax::fragment::{DEFAULT_MAX_DEPTH, DecomposedFile, FragmentKind, SymbolKind};
use crate::test_support::{load_fixture, registry};

#[fixture]
fn basic() -> DecomposedFile {
    let source = load_fixture("syntax/languages/python", "basic.py");
    let reg = registry();
    let d = reg.get("py").unwrap();
    let (result, _tree) = d.decompose(&source, DEFAULT_MAX_DEPTH);
    result
}

/// Top-level: 2 assignments, 1 function, 2 classes = 5 fragments.
#[rstest]
fn fragment_count(basic: DecomposedFile) {
    assert_eq!(basic.fragments.len(), 5);
}

#[rstest]
fn fragment_names(basic: DecomposedFile) {
    let names: Vec<_> = basic.fragments.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, &["MAX_RETRIES", "DEFAULT_NAME", "greet", "Config", "Processor"]);
}

#[rstest]
fn fragment_kinds(basic: DecomposedFile) {
    let kinds: Vec<_> = basic.fragments.iter().map(|f| &f.kind).collect();
    assert_eq!(kinds, &[
        &FragmentKind::Symbol(SymbolKind::Variable),
        &FragmentKind::Symbol(SymbolKind::Variable),
        &FragmentKind::Symbol(SymbolKind::Function),
        &FragmentKind::Symbol(SymbolKind::Class),
        &FragmentKind::Symbol(SymbolKind::Class),
    ]);
}

/// Imports are coalesced into a single ImportSpan.
#[rstest]
fn imports_extracted(basic: DecomposedFile) {
    let imports = basic.imports.as_ref().expect("imports should be present");
    assert!(imports.content.contains("import os"));
    assert!(imports.content.contains("from pathlib import Path"));
}

/// Class `Config` has field annotations and methods as children.
#[rstest]
fn class_children(basic: DecomposedFile) {
    let config = basic.fragments.iter().find(|f| f.name == "Config").unwrap();
    let child_names: Vec<_> = config.children.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(child_names, &["name", "debug", "validate", "reset"]);
}

/// Class `Processor` has 2 methods.
#[rstest]
fn processor_children(basic: DecomposedFile) {
    let processor = basic.fragments.iter().find(|f| f.name == "Processor").unwrap();
    let child_names: Vec<_> = processor.children.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(child_names, &["__init__", "run"]);
}
