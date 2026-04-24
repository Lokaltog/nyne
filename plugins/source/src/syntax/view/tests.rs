use std::sync::Arc;

use rstest::rstest;

use super::*;
use crate::syntax::SyntaxRegistry;
use crate::syntax::decomposed::{DecomposedSource, build_decomposed_source};
use crate::syntax::fragment::{FragmentKind, SymbolKind};
use crate::test_support::registry;

/// Load a fixture file by language extension and name.
fn load_fixture(name: &str) -> String { nyne::load_fixture!("syntax/view", name) }

/// Decompose a fixture file into a shared `DecomposedSource` for testing views.
fn decompose_fixture(reg: &SyntaxRegistry, ext: &str, name: &str) -> Arc<DecomposedSource> {
    build_decomposed_source(
        load_fixture(name),
        Arc::clone(reg.get(ext).expect("no decomposer for extension")),
        5,
    )
}

/// Extract a named `FragmentView` from the top-level fragments.
fn find_view(shared: &Arc<DecomposedSource>, name: &str) -> FragmentView {
    let frag = shared
        .decomposed
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("fragment '{name}' not found"));
    FragmentView {
        fragment: frag.clone(),
        shared: Arc::clone(shared),
    }
}

/// Verifies that `short_display` renders fragment kinds to expected display strings.
#[rstest]
#[case(FragmentKind::Symbol(SymbolKind::Function), "Function")]
#[case(FragmentKind::Symbol(SymbolKind::Struct), "Struct")]
#[case(FragmentKind::Symbol(SymbolKind::Trait), "Trait")]
#[case(FragmentKind::Section { level: 1 }, "h1")]
#[case(FragmentKind::Section { level: 3 }, "h3")]
#[case(FragmentKind::CodeBlock { lang: Some("rust".into()) }, "CodeBlock(rust)")]
#[case(FragmentKind::CodeBlock { lang: None }, "CodeBlock")]
fn short_display_renders(#[case] kind: FragmentKind, #[case] expected: &str) {
    assert_eq!(kind.short_display(), expected);
}

/// Verifies that `compact_visibility` shortens Rust visibility modifiers.
#[rstest]
#[case("pub", "pub")]
#[case("pub(crate)", "crate")]
#[case("pub(super)", "super")]
#[case("pub(in crate::foo)", "pub")]
#[case("", "")]
#[case("pub(self)", "pub(self)")]
fn compact_visibility_shortens(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(compact_visibility(input), expected);
}

/// Verifies that `code_block_summary` produces a summary from fixture code blocks.
#[test]
fn code_block_summary_from_fixture() {
    let reg = registry();
    let shared = decompose_fixture(&reg, "md", "section_with_code_blocks.md");
    let view = find_view(&shared, "Getting Started");
    // "Getting Started" has 2 code blocks: rust, sh
    assert_eq!(code_block_summary(&view.fragment.children), "2 blocks (rust, sh)");
}

/// Verifies that `code_block_summary` returns empty string for no children.
#[test]
fn code_block_summary_empty_children() {
    assert_eq!(code_block_summary(&[]), "");
}

/// Verifies that `code_block_summary` deduplicates non-adjacent language tags.
#[test]
fn code_block_summary_deduplicates_non_adjacent_langs() {
    let reg = registry();
    let shared = decompose_fixture(&reg, "md", "section_with_code_blocks.md");
    // Collect all code block children across all sections.
    let all_blocks: Vec<_> = shared
        .decomposed
        .iter()
        .flat_map(|f| &f.children)
        .filter(|c| matches!(c.kind, FragmentKind::CodeBlock { .. }))
        .cloned()
        .collect();
    // 3 code blocks total: rust, sh, rust — dedup must handle non-adjacent.
    let summary = code_block_summary(&all_blocks);
    assert_eq!(summary, "3 blocks (rust, sh)");
}

/// Verifies that `section_first_line` extracts the first non-heading content line.
#[rstest]
#[case("hello\nworld", Some("hello"))]
#[case("# Heading\nContent here", Some("Content here"))]
#[case("  \n  # H1\n  actual content  ", Some("actual content"))]
#[case("", None)]
#[case("# Only headings\n## More headings", None)]
#[case("  \n  \n  ", None)]
fn section_first_line_extracts(#[case] input: &str, #[case] expected: Option<&str>) {
    assert_eq!(section_first_line(input).as_deref(), expected);
}

/// Verifies that `description()` sources the correct text per fragment kind:
/// cleaned doc comment for symbols, empty when absent, first content line for sections.
#[rstest]
#[case::rust_doc_comment("rs", "documented_function.rs", "foo", "Does something cool.")]
#[case::rust_bare_function("rs", "bare_function.rs", "bar", "")]
#[case::markdown_section("md", "section_with_code_blocks.md", "Getting Started", "Welcome to the project.")]
fn description_cases(#[case] ext: &str, #[case] fixture: &str, #[case] name: &str, #[case] expected: &str) {
    let reg = registry();
    let shared = decompose_fixture(&reg, ext, fixture);
    assert_eq!(find_view(&shared, name).description(), expected);
}

/// Verifies `visibility()` across Rust visibility forms and non-applicable
/// (markdown section) fragment kinds.
#[rstest]
#[case::public("rs", "mixed_visibility.rs", "public_fn", "pub")]
#[case::crate_scoped("rs", "mixed_visibility.rs", "crate_fn", "crate")]
#[case::private("rs", "mixed_visibility.rs", "private_fn", "")]
#[case::markdown_section("md", "section_with_code_blocks.md", "API", "")]
fn visibility_cases(#[case] ext: &str, #[case] fixture: &str, #[case] name: &str, #[case] expected: &str) {
    let reg = registry();
    let shared = decompose_fixture(&reg, ext, fixture);
    assert_eq!(find_view(&shared, name).visibility(), expected);
}

/// Verifies that `fragment_list` filters out code block fragments.
#[test]
fn fragment_list_filters_code_blocks() {
    let reg = registry();
    let shared = decompose_fixture(&reg, "md", "section_with_code_blocks.md");
    let list = fragment_list(&shared.decomposed, &shared);
    // Should only contain sections, not code blocks.
    let seq = list.try_iter().expect("should be iterable");
    for item in seq {
        let is_code_block = item.get_attr("is_code_block").ok().map(|v| v.is_true());
        assert_ne!(is_code_block, Some(true), "code blocks should be filtered out");
    }
}
