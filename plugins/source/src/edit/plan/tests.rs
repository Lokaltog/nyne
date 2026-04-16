use super::*;
use crate::syntax::fragment::{Fragment, FragmentKind, FragmentSpan, SymbolKind};

/// Helper to build a code fragment with `fs_name` set.
///
/// `doc_range` optionally adds a Docstring child fragment.
fn code_fragment(
    name: &str,
    kind: SymbolKind,
    byte_range: std::ops::Range<usize>,
    doc_range: Option<std::ops::Range<usize>>,
    children: Vec<Fragment>,
) -> Fragment {
    let name_byte_offset = byte_range.start;
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
    Fragment {
        name: name.to_owned(),
        kind: FragmentKind::Symbol(kind),
        span: FragmentSpan::with_children(byte_range, name_byte_offset, &all_children),
        signature: Some(format!("fn {name}()")),
        visibility: None,
        metadata: None,
        children: all_children,
        parent_name: None,
        fs_name: Some(name.to_owned()),
    }
}

// Issue 3: Replace must use full_span (matching body.rs read range)

/// Verifies that replacing a symbol body includes the full span with doc comments.
#[test]
fn replace_body_uses_full_span_including_doc_comment() {
    // Source with a doc comment preceding a function.
    let source = "/// Doc comment\nfn foo() {\n    42\n}\n";
    //            ^0              ^16              ^34^36
    // full_span covers doc comment + body: 0..36
    // byte_range covers just the fn node: 16..36
    let mut frag = code_fragment("foo", SymbolKind::Function, 16..36, Some(0..15), vec![]);
    frag.signature = Some("fn foo()".to_owned());

    let plan = EditPlan {
        ops: vec![(0, EditOp {
            fragment_path: vec!["foo".to_owned()],
            kind: EditOpKind::Replace,
            content: Some("/// New doc\nfn foo() {\n    99\n}\n".to_owned()),
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

/// Verifies that replacing a body with identical content is a no-op.
#[test]
fn replace_body_round_trip_is_noop() {
    // Round-trip: reading the body (full_span) and writing it back via
    // edit/replace must be a no-op.
    let source = "/// Doc\nfn bar() {}\n";
    let mut frag = code_fragment("bar", SymbolKind::Function, 8..20, Some(0..7), vec![]);
    frag.signature = Some("fn bar()".to_owned());
    let frag = frag;

    // Simulate: body content = source[full_span] (what body.rs returns).
    let body_content = &source[0..20];

    let plan = EditPlan {
        ops: vec![(0, EditOp {
            fragment_path: vec!["bar".to_owned()],
            kind: EditOpKind::Replace,
            content: Some(body_content.to_owned()),
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

/// Verifies that appending into an empty impl block inserts correctly.
#[test]
fn append_into_empty_impl_block() {
    let source = "impl Foo {}\n";
    //            ^0         ^11^12
    let mut frag = code_fragment("Foo", SymbolKind::Impl, 0..12, None, vec![]);
    frag.signature = Some("impl Foo".to_owned());
    let frag = frag;

    let plan = EditPlan {
        ops: vec![(0, EditOp {
            fragment_path: vec!["Foo".to_owned()],
            kind: EditOpKind::Append,
            content: Some("    fn bar() {}\n".to_owned()),
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

/// Verifies that appending into an impl block with existing children works.
#[test]
fn append_into_scope_with_children_still_works() {
    let source = "impl Foo {\n    fn existing() {}\n}\n";
    //            ^0          ^11               ^30^31^32
    let mut child = code_fragment("existing", SymbolKind::Function, 15..29, None, vec![]);
    child.signature = Some("fn existing()".to_owned());
    child.parent_name = Some("Foo".to_owned());

    let mut frag = code_fragment("Foo", SymbolKind::Impl, 0..32, None, vec![child]);
    frag.signature = Some("impl Foo".to_owned());
    let frag = frag;

    let plan = EditPlan {
        ops: vec![(0, EditOp {
            fragment_path: vec!["Foo".to_owned()],
            kind: EditOpKind::Append,
            content: Some("    fn new_method() {}\n".to_owned()),
        })],
    };

    let resolved = plan.resolve(&[frag], source).unwrap();
    // Should insert after the last child's full_span.end (29).
    assert_eq!(resolved[0].byte_range.start, 29, "should insert after last child");

    let modified = EditPlan::apply(source, &resolved);
    assert!(modified.contains("fn existing()"), "existing method preserved");
    assert!(modified.contains("fn new_method()"), "new method appended");
}

/// Regression: `check_conflicts` must detect overlap when a zero-width insertion
/// and a non-empty edit share the same `byte_range.start`. Sort instability
/// could hide the conflict, letting `apply()` panic with begin > end.
#[test]
fn check_conflicts_detects_zero_width_at_same_start_as_nonempty() {
    let source = "/// Doc A\nfn aaa() {}\n/// Doc B\nfn bbb() {}\n";
    //            ^0        ^10        ^22        ^32        ^44

    // Two adjacent functions: aaa (0..22 full_span), bbb (22..44 full_span)
    let frag_a = code_fragment("aaa", SymbolKind::Function, 10..22, Some(0..9), vec![]);
    let frag_b = code_fragment("bbb", SymbolKind::Function, 32..44, Some(22..31), vec![]);

    // InsertAfter on aaa (zero-width at 22) + Replace on bbb (starts at 22).
    // These share byte_range.start = 22 and must be detected as a conflict
    // (the insert is inside the replaced range's start boundary).
    let plan = EditPlan {
        ops: vec![
            (0, EditOp {
                fragment_path: vec!["aaa".to_owned()],
                kind: EditOpKind::InsertAfter,
                content: Some("fn inserted() {}\n".to_owned()),
            }),
            (1, EditOp {
                fragment_path: vec!["bbb".to_owned()],
                kind: EditOpKind::Replace,
                content: Some("/// New B\nfn bbb() { 1 }\n".to_owned()),
            }),
        ],
    };

    let resolved = plan.resolve(&[frag_a, frag_b], source).unwrap();
    // Must not panic — the resolved edits should be applicable.
    let modified = EditPlan::apply(source, &resolved);
    // The insert-after content should appear between aaa and bbb.
    assert!(modified.contains("fn inserted()"), "inserted function present");
    assert!(modified.contains("fn bbb()"), "replaced function present");
}

/// `apply` requires edits pre-sorted ascending with zero-width insertions
/// before non-empty edits at the same offset — matching the ordering
/// produced by [`EditPlan::resolve`] via [`ResolvedEdit::cmp_ascending`].
#[test]
fn apply_handles_zero_width_and_nonempty_at_same_offset() {
    let source = "aaabbbccc";
    let edits = vec![
        ResolvedEdit {
            staged_index: 0,
            byte_range: 3..3,
            replacement: "INSERT".to_owned(),
        },
        ResolvedEdit {
            staged_index: 1,
            byte_range: 3..6,
            replacement: "BBB".to_owned(),
        },
    ];
    let result = EditPlan::apply(source, &edits);
    assert_eq!(result, "aaaINSERTBBBccc");
}
