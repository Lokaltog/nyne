use super::*;
use crate::syntax::fragment::{Fragment, FragmentKind, SymbolKind};

/// Helper to build a code fragment with `fs_name` set.
///
/// `doc_range` optionally adds a Docstring child fragment.
fn code_fragment(
    _source: &str,
    name: &str,
    kind: SymbolKind,
    byte_range: std::ops::Range<usize>,
    doc_range: Option<std::ops::Range<usize>>,
    children: Vec<Fragment>,
) -> Fragment {
    let name_offset = byte_range.start;
    let mut all_children = Vec::new();
    if let Some(dr) = doc_range {
        all_children.push(Fragment::structural(
            "docstring",
            FragmentKind::Docstring,
            dr,
            Some(name.to_owned()),
        ));
    }
    all_children.extend(children);
    let mut frag = Fragment::new(
        name.to_owned(),
        FragmentKind::Symbol(kind),
        byte_range,
        Some(format!("fn {name}()")),
        None,
        None,
        name_offset,
        all_children,
        None,
    );
    frag.fs_name = Some(name.to_owned());
    frag
}

// Issue 3: Replace must use full_span (matching body.rs read range)

#[test]
fn replace_body_uses_full_span_including_doc_comment() {
    // Source with a doc comment preceding a function.
    let source = "/// Doc comment\nfn foo() {\n    42\n}\n";
    //            ^0              ^16              ^34^36
    // full_span covers doc comment + body: 0..36
    // byte_range covers just the fn node: 16..36
    let mut frag = code_fragment(source, "foo", SymbolKind::Function, 16..36, Some(0..15), vec![]);
    frag.signature = Some("fn foo()".to_owned());

    let plan = EditPlan {
        ops: vec![(0, EditOp::Replace {
            fragment_path: vec!["foo".to_owned()],
            content: "/// New doc\nfn foo() {\n    99\n}\n".to_owned(),
        })],
    };

    let resolved = plan.resolve(&[frag], source).unwrap();
    assert_eq!(resolved.len(), 1);

    // The resolved range must cover the full_span (0..36), not byte_range (16..36).
    // line_start_of(source, 0) == 0.
    assert_eq!(
        resolved[0].byte_range,
        0..36,
        "Replace should target full_span, not byte_range"
    );

    let modified = EditPlan::apply(source, &resolved);
    assert_eq!(modified, "/// New doc\nfn foo() {\n    99\n}\n");
}

#[test]
fn replace_body_round_trip_is_noop() {
    // Round-trip: reading the body (full_span) and writing it back via
    // edit/replace must be a no-op.
    let source = "/// Doc\nfn bar() {}\n";
    let mut frag = code_fragment(source, "bar", SymbolKind::Function, 8..20, Some(0..7), vec![]);
    frag.signature = Some("fn bar()".to_owned());
    let frag = frag;

    // Simulate: body content = source[full_span] (what body.rs returns).
    let body_content = &source[0..20];

    let plan = EditPlan {
        ops: vec![(0, EditOp::Replace {
            fragment_path: vec!["bar".to_owned()],
            content: body_content.to_owned(),
        })],
    };

    let resolved = plan.resolve(&[frag], source).unwrap();
    let modified = EditPlan::apply(source, &resolved);
    assert_eq!(
        modified, source,
        "round-trip (cat body.rs > edit/replace) must be a no-op"
    );
}

// Issue 4: Append into empty scopes

#[test]
fn append_into_empty_impl_block() {
    let source = "impl Foo {}\n";
    //            ^0         ^11^12
    let mut frag = code_fragment(source, "Foo", SymbolKind::Impl, 0..12, None, vec![]);
    frag.signature = Some("impl Foo".to_owned());
    let frag = frag;

    let plan = EditPlan {
        ops: vec![(0, EditOp::Append {
            fragment_path: vec!["Foo".to_owned()],
            content: "    fn bar() {}\n".to_owned(),
        })],
    };

    let resolved = plan.resolve(&[frag], source).unwrap();
    assert_eq!(resolved.len(), 1);
    // Insertion point should be just before the closing brace (position 10).
    assert_eq!(resolved[0].byte_range.start, 10, "should insert before closing brace");

    let modified = EditPlan::apply(source, &resolved);
    // The result should have the method inside the braces.
    assert!(modified.contains("impl Foo {"), "impl header preserved");
    assert!(modified.contains("fn bar()"), "appended method present");
    assert!(modified.ends_with("}\n"), "closing brace preserved");
}

#[test]
fn append_into_scope_with_children_still_works() {
    let source = "impl Foo {\n    fn existing() {}\n}\n";
    //            ^0          ^11               ^30^31^32
    let mut child = code_fragment(source, "existing", SymbolKind::Function, 15..29, None, vec![]);
    child.signature = Some("fn existing()".to_owned());
    child.parent_name = Some("Foo".to_owned());

    let mut frag = code_fragment(source, "Foo", SymbolKind::Impl, 0..32, None, vec![child]);
    frag.signature = Some("impl Foo".to_owned());
    let frag = frag;

    let plan = EditPlan {
        ops: vec![(0, EditOp::Append {
            fragment_path: vec!["Foo".to_owned()],
            content: "    fn new_method() {}\n".to_owned(),
        })],
    };

    let resolved = plan.resolve(&[frag], source).unwrap();
    // Should insert after the last child's full_span.end (29).
    assert_eq!(resolved[0].byte_range.start, 29, "should insert after last child");

    let modified = EditPlan::apply(source, &resolved);
    assert!(modified.contains("fn existing()"), "existing method preserved");
    assert!(modified.contains("fn new_method()"), "new method appended");
}
