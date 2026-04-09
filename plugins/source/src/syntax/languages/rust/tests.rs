use nyne::load_fixture;
use rstest::{fixture, rstest};

use crate::syntax::fragment::{DecomposedFile, FragmentKind, SymbolKind};
use crate::test_support::registry;

/// Load `basic.rs` fixture source. Single source of truth for the fixture path.
fn load_basic() -> String { load_fixture!("syntax/languages/rust", "basic.rs") }

/// Fixture: decompose the basic.rs test file into fragments.
#[fixture]
fn basic() -> DecomposedFile {
    let (result, _tree) = registry().get("rs").unwrap().decompose(&load_basic(), 5);
    result
}

/// Top-level: imports + const + 2 fns + struct + enum + trait + 2 impls = 9 fragments.
#[rstest]
fn fragment_count(basic: DecomposedFile) {
    assert_eq!(basic.len(), 9);
}

/// Verifies that fragment names match the expected symbol names in order.
#[rstest]
fn fragment_names(basic: DecomposedFile) {
    let names: Vec<_> = basic.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, &[
        "imports",
        "MAX_SIZE",
        "process",
        "helper",
        "Config",
        "Status",
        "Processor",
        "Processor_for_Config",
        "Config"
    ]);
}

/// Verifies that fragment kinds match the expected symbol kinds in order.
#[rstest]
fn fragment_kinds(basic: DecomposedFile) {
    let kinds: Vec<_> = basic.iter().map(|f| &f.kind).collect();
    assert_eq!(kinds, &[
        &FragmentKind::Imports,
        &FragmentKind::Symbol(SymbolKind::Const),
        &FragmentKind::Symbol(SymbolKind::Function),
        &FragmentKind::Symbol(SymbolKind::Function),
        &FragmentKind::Symbol(SymbolKind::Struct),
        &FragmentKind::Symbol(SymbolKind::Enum),
        &FragmentKind::Symbol(SymbolKind::Trait),
        &FragmentKind::Symbol(SymbolKind::Impl),
        &FragmentKind::Symbol(SymbolKind::Impl),
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
    assert!(source[range.clone()].contains("use std::collections::HashMap;"));
    assert!(source[range].contains("use std::io;"));
}

/// Trait method signatures (`function_signature_item`) are not currently
/// decomposed as children — only method definitions with bodies are.
#[rstest]
fn trait_has_no_children_for_signatures(basic: DecomposedFile) {
    let processor_trait = basic.iter().find(|f| f.name == "Processor").unwrap();
    assert!(processor_trait.children.is_empty());
}

/// `impl Processor for Config` has 2 methods as children.
#[rstest]
fn trait_impl_children(basic: DecomposedFile) {
    let impl_frag = basic.iter().find(|f| f.name == "Processor_for_Config").unwrap();
    let child_names: Vec<_> = impl_frag.children.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(child_names, &["run", "reset"]);
}

/// `impl Config` has 1 method child.
#[rstest]
fn inherent_impl_children(basic: DecomposedFile) {
    let impl_frag = basic
        .iter()
        .filter(|f| f.name == "Config")
        .find(|f| f.kind == FragmentKind::Symbol(SymbolKind::Impl))
        .unwrap();
    let child_names: Vec<_> = impl_frag.children.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(child_names, &["new"]);
}
