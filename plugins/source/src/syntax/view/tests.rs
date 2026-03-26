use std::sync::Arc;

use rstest::rstest;

use super::*;
use crate::syntax::decomposed::DecomposedSource;
use crate::syntax::fragment::{DEFAULT_MAX_DEPTH, FragmentKind, SymbolKind};
use crate::syntax::{SyntaxRegistry, resolve_conflicts};
use crate::test_support::registry;

/// Load a fixture file by language extension and name.
fn load_fixture(name: &str) -> String { crate::test_support::load_fixture("syntax/view", name) }

/// Decompose a fixture file into a shared DecomposedSource for testing views.
fn decompose_fixture(reg: &SyntaxRegistry, ext: &str, name: &str) -> Arc<DecomposedSource> {
    let source = load_fixture(name);
    let decomposer = reg.get(ext).expect("no decomposer for extension");
    let (mut file, _tree) = decomposer.decompose(&source, DEFAULT_MAX_DEPTH);
    decomposer.map_to_fs(&mut file);
    resolve_conflicts(&mut file, decomposer);
    Arc::new(DecomposedSource {
        source,
        decomposed: file,
        decomposer: Arc::clone(decomposer),
        tree: None,
    })
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

/// Verifies that short_display renders fragment kinds to expected display strings.
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

/// Verifies that compact_visibility shortens Rust visibility modifiers.
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

/// Verifies that code_block_summary produces a summary from fixture code blocks.
#[test]
fn code_block_summary_from_fixture() {
    let reg = registry();
    let shared = decompose_fixture(&reg, "md", "section_with_code_blocks.md");
    let view = find_view(&shared, "Getting Started");
    // "Getting Started" has 2 code blocks: rust, sh
    assert_eq!(code_block_summary(&view.fragment.children), "2 blocks (rust, sh)");
}

/// Verifies that code_block_summary returns empty string for no children.
#[test]
fn code_block_summary_empty_children() {
    assert_eq!(code_block_summary(&[]), "");
}

/// Verifies that code_block_summary deduplicates non-adjacent language tags.
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

/// Verifies that section_first_line extracts the first non-heading content line.
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

/// Verifies that description returns the cleaned doc comment text.
#[test]
fn description_uses_doc_comment_via_decomposer() {
    let reg = registry();
    let shared = decompose_fixture(&reg, "rs", "documented_function.rs");
    let view = find_view(&shared, "foo");
    assert_eq!(view.description(), "Does something cool.");
}

/// Verifies that description returns empty string when no doc comment exists.
#[test]
fn description_empty_without_doc_comment() {
    let reg = registry();
    let shared = decompose_fixture(&reg, "rs", "bare_function.rs");
    let view = find_view(&shared, "bar");
    // No doc comment → empty description, no signature fallback.
    assert_eq!(view.description(), "");
}

/// Verifies that description returns the first content line for markdown sections.
#[test]
fn description_section_returns_first_content_line() {
    let reg = registry();
    let shared = decompose_fixture(&reg, "md", "section_with_code_blocks.md");
    let view = find_view(&shared, "Getting Started");
    assert_eq!(view.description(), "Welcome to the project.");
}

/// Verifies that visibility returns the compact form of the visibility modifier.
#[test]
fn visibility_returns_compact_form() {
    let reg = registry();
    let shared = decompose_fixture(&reg, "rs", "mixed_visibility.rs");

    let pub_view = find_view(&shared, "public_fn");
    assert_eq!(pub_view.visibility(), "pub");

    let crate_view = find_view(&shared, "crate_fn");
    assert_eq!(crate_view.visibility(), "crate");

    let private_view = find_view(&shared, "private_fn");
    assert_eq!(private_view.visibility(), "");
}

/// Verifies that visibility returns empty string for markdown sections.
#[test]
fn visibility_empty_for_sections() {
    let reg = registry();
    let shared = decompose_fixture(&reg, "md", "section_with_code_blocks.md");
    let view = find_view(&shared, "API");
    assert_eq!(view.visibility(), "");
}

/// Verifies that fragment_list filters out code block fragments.
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
