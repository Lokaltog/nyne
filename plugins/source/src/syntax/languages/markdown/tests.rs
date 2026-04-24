use rstest::rstest;

use crate::syntax::fragment::{DecomposedFile, FragmentKind};

crate::language_tests! {
    ext: "md",
    fixture_module: "syntax/languages/markdown",
    fixture_file: "basic.md",
    fragment_count: 3,
    fragment_names: ["preamble", "Introduction", "API Reference"],
    fragment_kinds: [
        FragmentKind::Preamble,
        FragmentKind::Section { level: 1 },
        FragmentKind::Section { level: 1 },
    ],
}

/// First fragment is a preamble containing frontmatter and intro text.
#[rstest]
fn preamble_is_first(basic: DecomposedFile) {
    let first = &basic[0];
    assert_eq!(first.name, "preamble");
    assert_eq!(first.kind, FragmentKind::Preamble);
}

/// Verifies Markdown section hierarchy: Introduction has nested Getting Started
/// (with its own Prerequisites child) and Configuration; API Reference has
/// Endpoints. Covers both top-level and nested (Getting Started is inside
/// Introduction) section child assertions.
#[rstest]
#[case::introduction("Introduction", &["Getting Started", "Configuration"])]
#[case::getting_started_nested("Getting Started", &["Prerequisites"])]
#[case::api_reference("API Reference", &["Endpoints"])]
fn section_children(basic: DecomposedFile, #[case] name: &str, #[case] expected: &[&str]) {
    crate::test_support::assert_fragment_children(&basic, name, expected);
}

/// No imports in Markdown.
#[rstest]
fn no_imports(basic: DecomposedFile) {
    use crate::syntax::fragment::find_fragment_of_kind;
    assert!(find_fragment_of_kind(&basic, &FragmentKind::Imports).is_none());
}
