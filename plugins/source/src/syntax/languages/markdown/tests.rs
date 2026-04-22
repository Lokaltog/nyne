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

/// Section hierarchy: Introduction has children Getting Started and Configuration.
/// Getting Started has child Prerequisites.
#[rstest]
fn introduction_children(basic: DecomposedFile) {
    let intro = basic.iter().find(|f| f.name == "Introduction").unwrap();
    let child_names: Vec<_> = intro.children.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(child_names, &["Getting Started", "Configuration"]);

    let getting_started = intro.children.iter().find(|f| f.name == "Getting Started").unwrap();
    let gs_child_names: Vec<_> = getting_started.children.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(gs_child_names, &["Prerequisites"]);
}

/// API Reference has one child: Endpoints.
#[rstest]
fn api_reference_children(basic: DecomposedFile) {
    let api = basic.iter().find(|f| f.name == "API Reference").unwrap();
    let child_names: Vec<_> = api.children.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(child_names, &["Endpoints"]);
}

/// No imports in Markdown.
#[rstest]
fn no_imports(basic: DecomposedFile) {
    use crate::syntax::fragment::find_fragment_of_kind;
    assert!(find_fragment_of_kind(&basic, &FragmentKind::Imports).is_none());
}
