use rstest::rstest;

use crate::syntax::fragment::{DecomposedFile, FragmentKind, SymbolKind};

crate::language_tests! {
    ext: "py",
    fixture_module: "syntax/languages/python",
    fixture_file: "basic.py",
    fragment_count: 6,
    fragment_names: [
        "imports",
        "MAX_RETRIES",
        "DEFAULT_NAME",
        "greet",
        "Config",
        "Processor",
    ],
    fragment_kinds: [
        FragmentKind::Imports,
        FragmentKind::Symbol(SymbolKind::Variable),
        FragmentKind::Symbol(SymbolKind::Variable),
        FragmentKind::Symbol(SymbolKind::Function),
        FragmentKind::Symbol(SymbolKind::Class),
        FragmentKind::Symbol(SymbolKind::Class),
    ],
    imports_contain: ["import os", "from pathlib import Path"],
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
