use rstest::{fixture, rstest};

use crate::syntax::fragment::{DEFAULT_MAX_DEPTH, DecomposedFile, FragmentKind};
use crate::test_support::{load_fixture, registry};

/// Fixture: decompose the basic.md test file into fragments.
#[fixture]
fn basic() -> DecomposedFile {
    let source = load_fixture("syntax/languages/markdown", "basic.md");
    let reg = registry();
    let d = reg.get("md").unwrap();
    let (result, _tree) = d.decompose(&source, DEFAULT_MAX_DEPTH);
    result
}

/// Top-level: preamble (frontmatter + pre-heading content) +
/// 2 h1 sections (Introduction, API Reference) = 3 fragments.
#[rstest]
fn fragment_count(basic: DecomposedFile) {
    assert_eq!(basic.len(), 3);
}

/// Verifies that fragment names match the expected section names in order.
#[rstest]
fn fragment_names(basic: DecomposedFile) {
    let names: Vec<_> = basic.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, &["preamble", "Introduction", "API Reference"]);
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
