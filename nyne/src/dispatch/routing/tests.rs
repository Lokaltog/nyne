use rstest::rstest;

use super::builder::{RouteNodeBuilder, RouteTreeBuilder};
use super::ctx::RouteCtx;
use super::params::RouteParams;
use super::segment::SegmentMatcher;
use crate::node::VirtualNode;
use crate::node::builtins::StaticContent;
use crate::provider::Nodes;
use crate::test_support::*;

/// Tests that exact segment matching is case-sensitive and requires full equality.
#[rstest]
#[case("foo", "foo", true)]
#[case("foo", "bar", false)]
#[case("foo", "FOO", false)]
fn exact_matching(#[case] pattern: &str, #[case] input: &str, #[case] expected: bool) {
    // Exact requires &'static str; leak for test convenience
    let pattern: &'static str = pattern.to_owned().leak();
    let m = SegmentMatcher::Exact(pattern);
    assert_eq!(m.matches(input).is_some(), expected);
}

/// Tests single-segment capture matching with optional prefix and suffix.
#[rstest]
#[case("x", None, None, "hello", Some("hello"))]
#[case("x", None, Some("@"), "hello@", Some("hello"))]
#[case("x", None, Some("@"), "hello", None)]
#[case("x", None, Some("@"), "@", None)]
#[case("x", Some("BLAME.md:"), None, "BLAME.md:5-20", Some("5-20"))]
#[case("x", Some("BLAME.md:"), None, "LOG.md:5-20", None)]
#[case("x", Some("BLAME.md:"), None, "BLAME.md:", None)]
#[case("x", Some("pre-"), Some("-suf"), "pre-hello-suf", Some("hello"))]
fn capture_matching(
    #[case] name: &'static str,
    #[case] prefix: Option<&'static str>,
    #[case] suffix: Option<&'static str>,
    #[case] input: &str,
    #[case] expected: Option<&str>,
) {
    use super::segment::CaptureResult;
    let m = SegmentMatcher::Capture { name, prefix, suffix };
    let result = m.matches(input);
    match expected {
        Some(val) => {
            let CaptureResult::Single(_, captured) = result.unwrap() else {
                panic!("expected single capture");
            };
            assert_eq!(captured, val);
        }
        None => assert!(result.is_none()),
    }
}

/// Verifies that Glob, Root, and RestCapture are not handled by `matches()`.
#[test]
fn glob_root_rest_not_matched_by_matches() {
    // These are handled by the tree walk, not by matches()
    assert!(SegmentMatcher::Glob.matches("anything").is_none());
    assert!(SegmentMatcher::Root.matches("x").is_none());
    assert!(
        SegmentMatcher::RestCapture {
            name: "x",
            suffix: None
        }
        .matches("x")
        .is_none()
    );
}

/// Tests that segment matcher precedence values follow the expected ordering.
#[rstest]
#[case(SegmentMatcher::Exact("x"), 1)]
#[case(SegmentMatcher::Capture { name: "x", prefix: Some("p:"), suffix: None }, 2)]
#[case(SegmentMatcher::Capture { name: "x", prefix: None, suffix: None }, 3)]
#[case(SegmentMatcher::RestCapture { name: "x", suffix: None }, 4)]
#[case(SegmentMatcher::Glob, 5)]
#[case(SegmentMatcher::Root, 0)]
fn precedence_ordering(#[case] matcher: SegmentMatcher, #[case] expected: u8) {
    assert_eq!(matcher.precedence(), expected);
}

/// Tests that a single-segment capture can be retrieved by name.
#[test]
fn param_returns_captured_value() {
    let mut params = RouteParams::default();
    params.insert_single("source", "file.rs".into());
    assert_eq!(params.get("source"), "file.rs");
}

/// Verifies that accessing a missing single capture panics.
#[test]
#[should_panic(expected = "no capture named 'missing'")]
fn param_panics_on_missing_name() {
    let params = RouteParams::default();
    params.get("missing");
}

/// Tests that a rest capture returns all captured segments.
#[test]
fn rest_param_returns_captured_segments() {
    let mut params = RouteParams::default();
    params.insert_rest("path", vec!["a".into(), "b".into()]);
    assert_eq!(params.get_rest("path"), &["a", "b"]);
}

/// Verifies that accessing a missing rest capture panics.
#[test]
#[should_panic(expected = "no rest capture named 'missing'")]
fn rest_param_panics_on_missing_name() {
    let params = RouteParams::default();
    params.get_rest("missing");
}

/// Stub provider used to test route tree dispatch.
struct TestProvider;

/// Handler methods for testing route tree dispatch.
impl TestProvider {
    /// Return a single `root.txt` file node.
    fn handle_root(&self, _ctx: &RouteCtx<'_>) -> Nodes {
        Ok(Some(vec![VirtualNode::file("root.txt", StaticContent(b"root"))]))
    }

    /// Return a file node named after the captured `x` parameter.
    fn handle_items(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let x = ctx.param("x");
        Ok(Some(vec![VirtualNode::file(
            format!("{x}.txt"),
            StaticContent(b"item"),
        )]))
    }

    /// Return a file node named after the joined rest-capture `xs` segments.
    fn handle_nested(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let xs = ctx.params("xs");
        Ok(Some(vec![VirtualNode::file(
            format!("{}.txt", xs.join("/")),
            StaticContent(b"nested"),
        )]))
    }
}

/// Tests that the tree dispatches children for an exact root-level match.
#[test]
fn tree_matches_exact_root() {
    let tree = RouteTreeBuilder::<TestProvider>::new()
        .route(RouteNodeBuilder::exact("foo").children(|p: &TestProvider, ctx| p.handle_root(ctx)))
        .build();

    let b = stub_request_context_at("foo");
    let result = tree.children(&TestProvider, &b.ctx()).unwrap();
    assert!(result.is_some());
}

/// Tests that the tree returns `None` for paths that don't match any route.
#[test]
fn tree_returns_none_for_unmatched_path() {
    let tree = RouteTreeBuilder::<TestProvider>::new()
        .route(RouteNodeBuilder::exact("foo").children(|p: &TestProvider, ctx| p.handle_root(ctx)))
        .build();

    let b = stub_request_context_at("bar");
    let result = tree.children(&TestProvider, &b.ctx()).unwrap();
    assert!(result.is_none());
}

/// Tests that captures from parent segments propagate to child route handlers.
#[test]
fn tree_captures_propagate_to_children() {
    let tree = RouteTreeBuilder::<TestProvider>::new()
        .route(
            RouteNodeBuilder::capture("x", None, Some("@"))
                .route(RouteNodeBuilder::exact("sub").children(|p: &TestProvider, ctx| p.handle_items(ctx))),
        )
        .build();

    let b = stub_request_context_at("hello@/sub");
    let result = tree.children(&TestProvider, &b.ctx()).unwrap().unwrap();
    assert_eq!(result.first().unwrap().name(), "hello.txt");
}

/// Tests that exact matches take precedence over capture matches.
#[test]
fn precedence_exact_before_capture() {
    let tree = RouteTreeBuilder::<TestProvider>::new()
        .route(
            RouteNodeBuilder::capture("x", None, None).children(|_p: &TestProvider, _ctx| {
                Ok(Some(vec![VirtualNode::file("capture.txt", StaticContent(b"cap"))]))
            }),
        )
        .route(RouteNodeBuilder::exact("foo").children(|_p: &TestProvider, _ctx| {
            Ok(Some(vec![VirtualNode::file("exact.txt", StaticContent(b"exact"))]))
        }))
        .build();

    let b = stub_request_context_at("foo");
    let result = tree.children(&TestProvider, &b.ctx()).unwrap().unwrap();
    assert_eq!(result.first().unwrap().name(), "exact.txt");
}

/// Tests that rest-capture with suffix strips it from all captured segments.
#[test]
fn rest_capture_rightmost_suffix() {
    let tree = RouteTreeBuilder::<TestProvider>::new()
        .route(
            RouteNodeBuilder::rest_capture("xs", Some("@"))
                .route(RouteNodeBuilder::exact("sub").children(|p: &TestProvider, ctx| p.handle_nested(ctx))),
        )
        .build();

    let b = stub_request_context_at("A@/mid/B@/sub");
    let result = tree.children(&TestProvider, &b.ctx()).unwrap().unwrap();
    // Suffix must be stripped from ALL captured segments, not just the terminal.
    assert_eq!(result.first().unwrap().name(), "A/mid/B.txt");
}

/// Verifies that all segments in a rest-capture have the suffix stripped.
#[test]
fn rest_capture_suffix_stripped_from_all_segments() {
    let tree = RouteTreeBuilder::<TestProvider>::new()
        .route(RouteNodeBuilder::rest_capture("xs", Some("@")).children(|p: &TestProvider, ctx| p.handle_nested(ctx)))
        .build();

    // All segments carry the suffix — all should be stripped.
    let b = stub_request_context_at("sec-a@/sec-b@/sec-c@");
    let result = tree.children(&TestProvider, &b.ctx()).unwrap().unwrap();
    assert_eq!(result.first().unwrap().name(), "sec-a/sec-b/sec-c.txt");
}

/// Tests that a rest-capture without suffix consumes all remaining segments.
#[test]
fn rest_capture_no_suffix_consumes_all() {
    let tree = RouteTreeBuilder::<TestProvider>::new()
        .route(RouteNodeBuilder::rest_capture("xs", None).children(|p: &TestProvider, ctx| p.handle_nested(ctx)))
        .build();

    let b = stub_request_context_at("a/b/c");
    let result = tree.children(&TestProvider, &b.ctx()).unwrap().unwrap();
    assert_eq!(result.first().unwrap().name(), "a/b/c.txt");
}

/// Tests that static files declared via `.file()` appear in children results.
#[test]
fn static_files_appear_in_children() {
    let tree = RouteTreeBuilder::<TestProvider>::new()
        .route(
            RouteNodeBuilder::exact("dir")
                .file("README.md", || VirtualNode::file("README.md", StaticContent(b"readme"))),
        )
        .build();

    let b = stub_request_context_at("dir");
    let result = tree.children(&TestProvider, &b.ctx()).unwrap().unwrap();
    assert_eq!(result.first().unwrap().name(), "README.md");
}

/// Tests that static files are discoverable via lookup.
#[test]
fn static_files_found_by_lookup() {
    let tree = RouteTreeBuilder::<TestProvider>::new()
        .route(
            RouteNodeBuilder::exact("dir")
                .file("README.md", || VirtualNode::file("README.md", StaticContent(b"readme"))),
        )
        .build();

    let b = stub_request_context_at("dir");
    let result = tree.lookup(&TestProvider, &b.ctx(), "README.md").unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().name(), "README.md");
}

/// Tests that a lookup handler is invoked and returns matches or `None`.
#[test]
fn lookup_handler_dispatches() {
    let tree = RouteTreeBuilder::<TestProvider>::new()
        .route(RouteNodeBuilder::exact("dir").lookup(|_p: &TestProvider, _ctx, name| {
            if name == "found" {
                Ok(Some(VirtualNode::file("found", StaticContent(b"yes"))))
            } else {
                Ok(None)
            }
        }))
        .build();

    let b = stub_request_context_at("dir");
    assert!(tree.lookup(&TestProvider, &b.ctx(), "found").unwrap().is_some());
    assert!(tree.lookup(&TestProvider, &b.ctx(), "missing").unwrap().is_none());
}

/// Tests that a glob route's lookup handler acts as a catch-all fallback.
#[test]
fn glob_lookup_fallback() {
    let tree = RouteTreeBuilder::<TestProvider>::new()
        .route(
            RouteNodeBuilder::glob()
                .lookup(|_p: &TestProvider, _ctx, name| Ok(Some(VirtualNode::file(name, StaticContent(b"glob"))))),
        )
        .build();

    let b = stub_request_context_at("any/deep/path");
    let result = tree.lookup(&TestProvider, &b.ctx(), "test").unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().name(), "test");
}

/// Tests that `rebuild_node` can re-invoke handlers to find a specific node by name.
#[test]
fn rebuild_node_finds_by_name() {
    let tree = RouteTreeBuilder::<TestProvider>::new()
        .route(RouteNodeBuilder::exact("dir").children(|_p: &TestProvider, _ctx| {
            Ok(Some(vec![
                VirtualNode::file("a.txt", StaticContent(b"a")),
                VirtualNode::file("b.txt", StaticContent(b"b")),
            ]))
        }))
        .build();

    let b = stub_request_context_at("dir");
    let result = tree.rebuild_node(&TestProvider, &b.ctx(), "b.txt").unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().name(), "b.txt");
}

/// Tests that exact sub-routes auto-emit directory entries but captures do not.
#[test]
fn exact_sub_routes_auto_emit_directories() {
    let tree = RouteTreeBuilder::<TestProvider>::new()
        .route(
            RouteNodeBuilder::exact("parent")
                .file("README.md", || VirtualNode::file("README.md", StaticContent(b"readme")))
                .route(RouteNodeBuilder::exact("child-a"))
                .route(RouteNodeBuilder::exact("child-b"))
                .route(RouteNodeBuilder::capture("id", None, None)),
        )
        .build();

    let b = stub_request_context_at("parent");
    let result = tree.children(&TestProvider, &b.ctx()).unwrap().unwrap();
    let names: Vec<&str> = result.iter().map(VirtualNode::name).collect();
    // Static file + two exact sub-routes (capture should NOT auto-emit)
    assert_eq!(names, ["README.md", "child-a", "child-b"]);
}

/// Tests that `no_emit` hides a directory from parent readdir while its children remain accessible.
#[test]
fn no_emit_hides_root_companion_but_contents_visible() {
    // Simulates a provider with "@" at root (like claude/nyne/git).
    // The "@" dir itself should NOT appear in root readdir,
    // but its children ("agents") should appear when readdir-ing "@/".
    let tree = RouteTreeBuilder::<TestProvider>::new()
        .route(RouteNodeBuilder::exact(".claude"))
        .route(
            RouteNodeBuilder::exact("@")
                .no_emit()
                .route(RouteNodeBuilder::exact("agents")),
        )
        .build();

    // Root readdir: only ".claude", NOT "@"
    let b = stub_request_context_at("");
    let root_children = tree.children(&TestProvider, &b.ctx()).unwrap().unwrap();
    let root_names: Vec<&str> = root_children.iter().map(VirtualNode::name).collect();
    assert_eq!(root_names, [".claude"]);

    // But "@/" readdir: "agents" IS visible
    let b = stub_request_context_at("@");
    let at_children = tree.children(&TestProvider, &b.ctx()).unwrap().unwrap();
    let at_names: Vec<&str> = at_children.iter().map(VirtualNode::name).collect();
    assert_eq!(at_names, ["agents"]);
}

/// Tests that `no_emit` suppresses auto-directory emission for a sub-route.
#[test]
fn no_emit_suppresses_auto_directory() {
    let tree = RouteTreeBuilder::<TestProvider>::new()
        .route(
            RouteNodeBuilder::exact("parent")
                .route(RouteNodeBuilder::exact("visible"))
                .route(RouteNodeBuilder::exact("hidden").no_emit()),
        )
        .build();

    let b = stub_request_context_at("parent");
    let result = tree.children(&TestProvider, &b.ctx()).unwrap().unwrap();
    let names: Vec<&str> = result.iter().map(VirtualNode::name).collect();
    assert_eq!(names, ["visible"]);
}

/// Tests that a root-level children handler is invoked for the empty path.
#[test]
fn root_children_handler() {
    let tree = RouteTreeBuilder::<TestProvider>::new()
        .children(|_p: &TestProvider, _ctx| Ok(Some(vec![VirtualNode::directory("@")])))
        .build();

    let b = stub_request_context_at("");
    let result = tree.children(&TestProvider, &b.ctx()).unwrap().unwrap();
    assert_eq!(result.first().unwrap().name(), "@");
}
