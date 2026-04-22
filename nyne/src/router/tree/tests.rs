use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::Result;
use rstest::rstest;

use super::*;
use crate::router::{Chain, NamedNode, Next, Node, Op, Provider, Request};
use crate::test_support::{StubReadable, test_read_ctx};

struct TestProvider;

impl TestProvider {
    fn file_a(_: &Self, _ctx: &RouteCtx, _req: &Request) -> Option<NamedNode> {
        Some(
            Node::file()
                .with_readable(StubReadable::new("content-a"))
                .named("a.txt"),
        )
    }

    fn file_b(_: &Self, _ctx: &RouteCtx, _req: &Request) -> Option<NamedNode> {
        Some(
            Node::file()
                .with_readable(StubReadable::new("content-b"))
                .named("b.txt"),
        )
    }

    fn inner_txt(_: &Self, _ctx: &RouteCtx, _req: &Request) -> Option<NamedNode> {
        Some(
            Node::file()
                .with_readable(StubReadable::new("inner"))
                .named("inner.txt"),
        )
    }

    fn leaf_txt(_: &Self, _ctx: &RouteCtx, _req: &Request) -> Option<NamedNode> {
        Some(Node::file().with_readable(StubReadable::new("leaf")).named("leaf.txt"))
    }

    fn captured_body(_: &Self, ctx: &RouteCtx, _req: &Request) -> Option<NamedNode> {
        let param = ctx.param("name").unwrap_or("unknown");
        Some(
            Node::file()
                .with_readable(StubReadable::new(&format!("body of {param}")))
                .named("body.rs"),
        )
    }

    fn file_always(_: &Self, _ctx: &RouteCtx, _req: &Request) -> NamedNode {
        Node::file()
            .with_readable(StubReadable::new("always"))
            .named("always.txt")
    }

    fn dynamic_handler(_: &Self, _ctx: &RouteCtx, req: &mut Request, next: &Next) -> Result<()> {
        next.run(req)?;
        req.nodes.add(NamedNode::dir("dynamic-child"));
        Ok(())
    }
}

fn readdir_req(path: &str) -> Request { Request::new(PathBuf::from(path), Op::Readdir) }

#[test]
fn handler_runs_before_auto_emit() {
    let tree = RouteTree::builder()
        .handler(TestProvider::dynamic_handler)
        .content(TestProvider::file_a)
        .build();

    let provider = TestProvider;
    let next = Next::empty();
    let mut req = readdir_req("");

    tree.dispatch(&provider, &mut req, &next).unwrap();

    assert!(
        req.nodes.find("dynamic-child").is_some(),
        "handler should add dynamic-child"
    );
    assert!(req.nodes.find("a.txt").is_some(), "auto-emit should add a.txt");
}

// `content_always` standalone coverage is subsumed by
// `content_always_emits_like_content` below (which exercises both the
// Readdir and Lookup paths together with a sibling `content` producer).

#[test]
fn no_handler_defaults_to_next() {
    struct TreeProvider {
        tree: RouteTree<Self>,
    }
    crate::define_provider!(TreeProvider, "tree");
    impl Provider for TreeProvider {
        fn accept(&self, req: &mut Request, next: &Next) -> Result<()> { self.tree.dispatch(self, req, next) }
    }

    let chain = Chain::build(vec![
        Arc::new(TreeProvider {
            tree: RouteTree::builder().build(),
        }),
        Arc::new(crate::test_support::StoppingProvider::new()),
    ])
    .unwrap();
    let mut req = readdir_req("");

    chain.dispatch(&mut req).unwrap();

    assert!(
        req.nodes.find("stopped.txt").is_some(),
        "next should have been called — stopper must produce stopped.txt"
    );
}

#[test]
fn static_dir_takes_priority_over_capture() {
    let tree = RouteTree::builder()
        .dir("special", |d| d.content(TestProvider::file_a))
        .capture("name", |d| d.content(TestProvider::file_b))
        .build();

    let provider = TestProvider;
    let next = Next::empty();
    let mut req = readdir_req("special");

    tree.dispatch(&provider, &mut req, &next).unwrap();

    assert!(req.nodes.find("a.txt").is_some(), "should descend into static dir");
    assert!(req.nodes.find("b.txt").is_none(), "should NOT descend into capture");
}

#[test]
fn no_match_passthrough() {
    struct TreeProvider {
        tree: RouteTree<Self>,
    }
    impl TreeProvider {
        fn leaf(_: &Self, _ctx: &RouteCtx, _req: &Request) -> Option<NamedNode> { Some(Node::file().named("a.txt")) }
    }
    crate::define_provider!(TreeProvider, "tree");
    impl Provider for TreeProvider {
        fn accept(&self, req: &mut Request, next: &Next) -> Result<()> { self.tree.dispatch(self, req, next) }
    }

    let tree = RouteTree::builder()
        .dir("known", |d| d.content(TreeProvider::leaf))
        .build();
    let chain = Chain::build(vec![
        Arc::new(TreeProvider { tree }),
        Arc::new(crate::test_support::StoppingProvider::new()),
    ])
    .unwrap();
    let mut req = readdir_req("unknown/path");

    chain.dispatch(&mut req).unwrap();

    assert!(
        req.nodes.find("stopped.txt").is_some(),
        "unmatched path should passthrough to next — stopper must produce stopped.txt"
    );
}

#[test]
fn readdir_does_not_emit_captures() {
    let tree = RouteTree::builder()
        .content(TestProvider::file_a)
        .capture("name", |d| d.content(TestProvider::captured_body))
        .rest("rest", |d| d)
        .build();

    let provider = TestProvider;
    let next = Next::empty();
    let mut req = readdir_req("");

    tree.dispatch(&provider, &mut req, &next).unwrap();

    assert!(req.nodes.find("a.txt").is_some(), "should emit content node");
    assert_eq!(
        req.nodes.len(),
        1,
        "should only emit content entries, not captures/rest"
    );
}

#[rstest]
#[case::readdir(Op::Readdir, &["a.txt", "b.txt", "sub"])]
#[case::lookup_leaf(Op::Lookup { name: "a.txt".into() }, &["a.txt"])]
#[case::lookup_subtree(Op::Lookup { name: "sub".into() }, &["sub"])]
#[case::lookup_miss(Op::Lookup { name: "nope".into() }, &[])]
#[case::create_no_emit(Op::Create { name: "x".into() }, &[])]
#[case::remove_no_emit(Op::Remove { name: "x".into() }, &[])]
#[case::mkdir_no_emit(Op::Mkdir { name: "x".into() }, &[])]
#[case::rename_no_emit(Op::Rename { src_name: "a.txt".into(), target_dir: PathBuf::from(""), target_name: "z.txt".into() }, &[])]
fn auto_emit_by_op(#[case] op: Op, #[case] expected_names: &[&str]) {
    let tree = RouteTree::builder()
        .content(TestProvider::file_a)
        .content(TestProvider::file_b)
        .dir("sub", |d| d)
        .build();

    let provider = TestProvider;
    let next = Next::empty();
    let mut req = Request::new(PathBuf::new(), op);

    tree.dispatch(&provider, &mut req, &next).unwrap();

    let mut names: Vec<&str> = req.nodes.iter().map(NamedNode::name).collect();
    names.sort_unstable();
    let mut expected: Vec<&str> = expected_names.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);
}

#[rstest]
#[case::root("", &["a.txt", "sub"])]
#[case::subtree("sub", &["deep", "inner.txt"])]
#[case::nested("sub/deep", &["leaf.txt"])]
fn dispatch_at_depth(#[case] path: &str, #[case] expected_names: &[&str]) {
    let tree = RouteTree::builder()
        .content(TestProvider::file_a)
        .dir("sub", |d| {
            d.content(TestProvider::inner_txt)
                .dir("deep", |d| d.content(TestProvider::leaf_txt))
        })
        .build();

    let provider = TestProvider;
    let next = Next::empty();
    let mut req = readdir_req(path);

    tree.dispatch(&provider, &mut req, &next).unwrap();

    let mut names: Vec<&str> = req.nodes.iter().map(NamedNode::name).collect();
    names.sort_unstable();
    let mut expected: Vec<&str> = expected_names.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);
}

#[rstest]
#[case::simple_capture("Foo", "name", "Foo")]
#[case::nested_capture("Foo/sub", "name", "Foo")]
fn capture_binds_param(#[case] path: &str, #[case] param: &str, #[case] expected: &str) {
    let tree = RouteTree::builder()
        .capture("name", |d| {
            d.content(TestProvider::captured_body)
                .dir("sub", |d| d.content(TestProvider::captured_body))
        })
        .build();

    let provider = TestProvider;
    let next = Next::empty();
    let mut req = readdir_req(path);

    tree.dispatch(&provider, &mut req, &next).unwrap();

    let body_node = req.nodes.find("body.rs").expect("should have body.rs");
    let content = body_node
        .readable()
        .expect("body.rs must be readable")
        .read(&test_read_ctx())
        .expect("read must succeed");
    let text = std::str::from_utf8(&content).unwrap();
    assert!(
        text.contains(expected),
        "expected param {param}={expected} in content: {text}"
    );
}

#[test]
fn dispatch_when_passes_through_without_state() {
    #[derive(Clone)]
    struct Marker;

    struct TreeProvider {
        tree: RouteTree<Self>,
    }
    impl TreeProvider {
        fn leaf(_: &Self, _ctx: &RouteCtx, _req: &Request) -> Option<NamedNode> {
            Some(Node::file().named("should_not_appear.txt"))
        }
    }
    crate::define_provider!(TreeProvider, "tree");
    impl Provider for TreeProvider {
        fn accept(&self, req: &mut Request, next: &Next) -> Result<()> {
            self.tree.dispatch_when::<Marker>(self, req, next)
        }
    }

    let chain = Chain::build(vec![
        Arc::new(TreeProvider {
            tree: RouteTree::builder().content(TreeProvider::leaf).build(),
        }),
        Arc::new(crate::test_support::StoppingProvider::new()),
    ])
    .unwrap();
    let mut req = readdir_req("");

    chain.dispatch(&mut req).unwrap();

    assert!(
        req.nodes.find("should_not_appear.txt").is_none(),
        "tree should not dispatch when state is absent"
    );
    assert!(
        req.nodes.find("stopped.txt").is_some(),
        "next should have been called — stopper must produce stopped.txt"
    );
}

#[test]
fn dispatch_when_dispatches_with_state() {
    #[derive(Clone)]
    struct Marker;

    struct TreeProvider {
        tree: RouteTree<Self>,
    }
    impl TreeProvider {
        fn leaf(_: &Self, _ctx: &RouteCtx, _req: &Request) -> Option<NamedNode> {
            Some(Node::file().named("present.txt"))
        }
    }
    crate::define_provider!(TreeProvider, "tree");
    impl Provider for TreeProvider {
        fn accept(&self, req: &mut Request, next: &Next) -> Result<()> {
            self.tree.dispatch_when::<Marker>(self, req, next)
        }
    }

    let chain = Chain::build(vec![Arc::new(TreeProvider {
        tree: RouteTree::builder().content(TreeProvider::leaf).build(),
    })])
    .unwrap();
    let mut req = readdir_req("");
    req.set_state(Marker);

    chain.dispatch(&mut req).unwrap();

    assert!(
        req.nodes.find("present.txt").is_some(),
        "tree should dispatch when state is present"
    );
}
#[rstest]
#[case::readdir(Op::Readdir, &["from-callback"])]
#[case::lookup(Op::Lookup { name: "foo".into() }, &[])]
#[case::create(Op::Create { name: "foo".into() }, &[])]
fn on_readdir_fires_only_for_readdir(#[case] op: Op, #[case] expected: &[&str]) {
    struct P;
    impl P {
        fn add_items(_: &Self, _ctx: &RouteCtx, req: &mut Request) -> Result<()> {
            req.nodes.add(NamedNode::dir("from-callback"));
            Ok(())
        }
    }

    let tree = RouteTree::builder().on_readdir(P::add_items).build();

    let mut req = Request::new(PathBuf::new(), op);
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    // Lookup has a readdir-fallback branch (see `on_readdir_fallback_resolves_lookup`)
    // which filters by name — "from-callback" doesn't match "foo", so it's dropped.
    let names: Vec<&str> = req.nodes.iter().map(NamedNode::name).collect();
    assert_eq!(names, expected);
}

#[rstest]
#[case::lookup_hit(Op::Lookup { name: "target.txt".into() }, &["target.txt"])]
#[case::lookup_miss(Op::Lookup { name: "other".into() }, &[])]
#[case::remove_hit(Op::Remove { name: "target.txt".into() }, &["target.txt"])]
#[case::readdir_no_fire(Op::Readdir, &[])]
#[case::create_no_fire(Op::Create { name: "target.txt".into() }, &[])]
fn on_lookup_fires_with_name_for_lookup_and_remove(#[case] op: Op, #[case] expected: &[&str]) {
    struct P;
    impl P {
        fn resolve(_: &Self, _ctx: &RouteCtx, req: &mut Request, name: &str) -> Result<()> {
            if name == "target.txt" {
                req.nodes.add(Node::file().named("target.txt"));
            }
            Ok(())
        }
    }

    let tree = RouteTree::builder().on_lookup(P::resolve).build();

    let mut req = Request::new(PathBuf::new(), op);
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    let names: Vec<&str> = req.nodes.iter().map(NamedNode::name).collect();
    assert_eq!(names, expected);
}

#[test]
fn on_readdir_fallback_resolves_lookup() {
    struct P;
    impl P {
        fn list(_: &Self, _ctx: &RouteCtx, req: &mut Request) -> Result<()> {
            req.nodes.add(NamedNode::dir("alpha"));
            req.nodes.add(NamedNode::dir("beta"));
            Ok(())
        }
    }

    let tree = RouteTree::builder().on_readdir(P::list).build();

    // Lookup without on_lookup — should fall back to readdir + retain.
    let mut req = Request::new(PathBuf::new(), Op::Lookup { name: "beta".into() });
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    assert!(
        req.nodes.find("beta").is_some(),
        "readdir fallback should resolve lookup"
    );
    assert!(
        req.nodes.find("alpha").is_none(),
        "readdir fallback should filter non-matching nodes"
    );
}

// `on_lookup_does_not_fire_on_readdir` is subsumed by the
// `readdir_no_fire` / `create_no_fire` cases of
// `on_lookup_fires_with_name_for_lookup_and_remove` above.

#[rstest]
#[case::create_hit(Op::Create { name: "new-file".into() }, &["new-file"])]
#[case::create_miss(Op::Create { name: "other".into() }, &[])]
#[case::lookup_no_fire(Op::Lookup { name: "new-file".into() }, &[])]
#[case::readdir_no_fire(Op::Readdir, &[])]
#[case::remove_no_fire(Op::Remove { name: "new-file".into() }, &[])]
fn on_create_fires_only_for_create(#[case] op: Op, #[case] expected: &[&str]) {
    struct P;
    impl P {
        fn resolve(_: &Self, _ctx: &RouteCtx, req: &mut Request, name: &str) -> Result<()> {
            if name == "new-file" {
                req.nodes.add(Node::file().named("new-file"));
            }
            Ok(())
        }
    }

    let tree = RouteTree::builder().on_create(P::resolve).build();

    let mut req = Request::new(PathBuf::new(), op);
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    let names: Vec<&str> = req.nodes.iter().map(NamedNode::name).collect();
    assert_eq!(names, expected);
}

#[test]
fn handler_takes_priority_over_op_callbacks() {
    struct P;
    impl P {
        fn full_handler(_: &Self, _ctx: &RouteCtx, req: &mut Request, next: &Next) -> Result<()> {
            req.nodes.add(NamedNode::dir("from-handler"));
            next.run(req)
        }

        fn readdir_cb(_: &Self, _ctx: &RouteCtx, req: &mut Request) -> Result<()> {
            req.nodes.add(NamedNode::dir("from-callback"));
            Ok(())
        }
    }

    let tree = RouteTree::builder()
        .handler(P::full_handler)
        .on_readdir(P::readdir_cb)
        .build();

    let mut req = readdir_req("");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    assert!(req.nodes.find("from-handler").is_some(), "handler should run");
    assert!(
        req.nodes.find("from-callback").is_none(),
        "on_readdir should NOT run when handler is set"
    );
}

#[test]
fn on_op_fires_for_matching_op() {
    struct P;

    let tree = RouteTree::builder()
        .on_op(Op::is_rename, |_p, _ctx, req, _next| {
            req.nodes.add(NamedNode::dir("rename-handled"));
            Ok(())
        })
        .build();

    let mut req = Request::new(PathBuf::new(), Op::Rename {
        src_name: "a".into(),
        target_dir: PathBuf::new(),
        target_name: "b".into(),
    });
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    assert!(
        req.nodes.find("rename-handled").is_some(),
        "on_op should fire for matching op"
    );
}

#[test]
fn on_op_skips_non_matching_op() {
    struct P;
    impl P {
        fn add_items(_: &Self, _ctx: &RouteCtx, req: &mut Request) -> Result<()> {
            req.nodes.add(NamedNode::dir("from-readdir"));
            Ok(())
        }
    }

    let tree = RouteTree::builder()
        .on_op(Op::is_rename, |_p, _ctx, req, _next| {
            req.nodes.add(NamedNode::dir("rename-handled"));
            Ok(())
        })
        .on_readdir(P::add_items)
        .build();

    let mut req = readdir_req("");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    assert!(
        req.nodes.find("rename-handled").is_none(),
        "rename handler should not fire for readdir"
    );
    assert!(
        req.nodes.find("from-readdir").is_some(),
        "on_readdir should fire when on_op doesn't match"
    );
}

#[test]
fn handler_takes_priority_over_on_op() {
    struct P;

    let tree = RouteTree::builder()
        .handler(|_p, _ctx, req, next| {
            req.nodes.add(NamedNode::dir("from-handler"));
            next.run(req)
        })
        .on_op(Op::is_rename, |_p, _ctx, req, _next| {
            req.nodes.add(NamedNode::dir("from-on-op"));
            Ok(())
        })
        .build();

    let mut req = Request::new(PathBuf::new(), Op::Rename {
        src_name: "a".into(),
        target_dir: PathBuf::new(),
        target_name: "b".into(),
    });
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    assert!(req.nodes.find("from-handler").is_some(), "handler should run");
    assert!(
        req.nodes.find("from-on-op").is_none(),
        "on_op should NOT run when handler is set"
    );
}

#[test]
fn on_op_takes_priority_over_op_callbacks() {
    struct P;
    impl P {
        fn readdir_cb(_: &Self, _ctx: &RouteCtx, req: &mut Request) -> Result<()> {
            req.nodes.add(NamedNode::dir("from-callback"));
            Ok(())
        }
    }

    let tree = RouteTree::builder()
        .on_op(Op::is_readdir, |_p, _ctx, req, next| {
            req.nodes.add(NamedNode::dir("from-on-op"));
            next.run(req)
        })
        .on_readdir(P::readdir_cb)
        .build();

    let mut req = readdir_req("");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    assert!(req.nodes.find("from-on-op").is_some(), "on_op should run");
    assert!(
        req.nodes.find("from-callback").is_none(),
        "on_readdir should NOT run when on_op matches"
    );
}

#[test]
fn on_op_first_match_wins() {
    struct P;

    let tree = RouteTree::builder()
        .on_op(Op::is_mutation, |_p, _ctx, req, _next| {
            req.nodes.add(NamedNode::dir("mutation-handler"));
            Ok(())
        })
        .on_op(Op::is_remove, |_p, _ctx, req, _next| {
            req.nodes.add(NamedNode::dir("remove-handler"));
            Ok(())
        })
        .build();

    let mut req = Request::new(PathBuf::new(), Op::Remove { name: "x".into() });
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    assert!(
        req.nodes.find("mutation-handler").is_some(),
        "first matching guard wins"
    );
    assert!(
        req.nodes.find("remove-handler").is_none(),
        "second guard should not fire"
    );
}

#[test]
fn on_op_receives_next_and_can_chain() {
    struct OpProvider {
        tree: RouteTree<Self>,
    }
    crate::define_provider!(OpProvider, "a-on-op");
    impl Provider for OpProvider {
        fn accept(&self, req: &mut Request, next: &Next) -> Result<()> { self.tree.dispatch(self, req, next) }
    }

    let chain = Chain::build(vec![
        Arc::new(OpProvider {
            tree: RouteTree::builder()
                .on_op(Op::is_create, |_p, _ctx, req, next| {
                    req.nodes.add(NamedNode::dir("before-next"));
                    next.run(req)?;
                    req.nodes.add(NamedNode::dir("after-next"));
                    Ok(())
                })
                .build(),
        }),
        Arc::new(crate::test_support::StoppingProvider::new()),
    ])
    .unwrap();

    let mut req = Request::new(PathBuf::new(), Op::Create { name: "x".into() });
    chain.dispatch(&mut req).unwrap();

    assert!(req.nodes.find("before-next").is_some(), "on_op should run before next");
    assert!(
        req.nodes.find("stopped.txt").is_some(),
        "next should be called — stopper must produce stopped.txt"
    );
    assert!(req.nodes.find("after-next").is_some(), "on_op should run after next");
}

#[test]
fn on_readdir_chains_next_automatically() {
    struct AProvider {
        tree: RouteTree<Self>,
    }
    impl AProvider {
        fn add_items(_: &Self, _ctx: &RouteCtx, req: &mut Request) -> Result<()> {
            req.nodes.add(NamedNode::dir("from-callback"));
            Ok(())
        }
    }
    crate::define_provider!(AProvider, "a-readdir");
    impl Provider for AProvider {
        fn accept(&self, req: &mut Request, next: &Next) -> Result<()> { self.tree.dispatch(self, req, next) }
    }

    let chain = Chain::build(vec![
        Arc::new(AProvider {
            tree: RouteTree::builder().on_readdir(AProvider::add_items).build(),
        }),
        Arc::new(crate::test_support::StoppingProvider::new()),
    ])
    .unwrap();

    let mut req = readdir_req("");
    chain.dispatch(&mut req).unwrap();

    assert!(
        req.nodes.find("stopped.txt").is_some(),
        "next should be called automatically — stopper must produce stopped.txt"
    );
    assert!(
        req.nodes.find("from-callback").is_some(),
        "callback should have contributed nodes"
    );
}

#[rstest]
#[case::readdir(Op::Readdir, &["always.txt", "a.txt"])]
#[case::lookup_hit(Op::Lookup { name: "always.txt".into() }, &["always.txt"])]
#[case::lookup_miss(Op::Lookup { name: "nope".into() }, &[])]
fn content_always_emits_like_content(#[case] op: Op, #[case] expected_names: &[&str]) {
    let tree = RouteTree::builder()
        .content_always(TestProvider::file_always)
        .content(TestProvider::file_a)
        .build();

    let provider = TestProvider;
    let next = Next::empty();
    let mut req = Request::new(PathBuf::new(), op);

    tree.dispatch(&provider, &mut req, &next).unwrap();

    let mut names: Vec<&str> = req.nodes.iter().map(NamedNode::name).collect();
    names.sort_unstable();
    let mut expected: Vec<&str> = expected_names.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);
}

#[rstest]
#[case::readdir("symbols/Foo", Op::Readdir, &["from-readdir"])]
#[case::lookup_hit("symbols/Foo", Op::Lookup { name: "from-readdir".into() }, &["from-readdir"])]
#[case::lookup_miss("symbols/Foo", Op::Lookup { name: "nope".into() }, &[])]
fn rest_subtree_dispatches_callbacks(#[case] path: &str, #[case] op: Op, #[case] expected_names: &[&str]) {
    struct P;
    impl P {
        fn add_items(_: &Self, ctx: &RouteCtx, req: &mut Request) -> Result<()> {
            assert_eq!(ctx.param("path"), Some("Foo"), "rest param should be captured");
            req.nodes.add(NamedNode::dir("from-readdir"));
            Ok(())
        }

        fn resolve(_: &Self, ctx: &RouteCtx, req: &mut Request, name: &str) -> Result<()> {
            assert_eq!(ctx.param("path"), Some("Foo"), "rest param should be captured");
            if name == "from-readdir" {
                req.nodes.add(NamedNode::dir("from-readdir"));
            }
            Ok(())
        }
    }

    let tree = RouteTree::builder()
        .dir("symbols", |d| {
            d.rest("path", |d| d.on_readdir(P::add_items).on_lookup(P::resolve))
        })
        .build();

    let mut req = Request::new(PathBuf::from(path), op);

    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    let mut names: Vec<&str> = req.nodes.iter().map(NamedNode::name).collect();
    names.sort_unstable();
    let mut expected: Vec<&str> = expected_names.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);
}

#[test]
fn same_named_dirs_merge_on_build() {
    struct P;
    impl P {
        fn file_a(_: &Self, _ctx: &RouteCtx, _req: &Request) -> Option<NamedNode> { Some(Node::file().named("a.txt")) }

        fn file_b(_: &Self, _ctx: &RouteCtx, _req: &Request) -> Option<NamedNode> { Some(Node::file().named("b.txt")) }
    }

    let tree = RouteTree::builder()
        .dir("shared", |d| d.content(P::file_a))
        .dir("shared", |d| d.content(P::file_b))
        .build();

    // readdir at root: only one "shared" entry (not two)
    let mut req = readdir_req("");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();
    let dir_count = req.nodes.iter().filter(|n| n.name() == "shared").count();
    assert_eq!(dir_count, 1, "same-named dirs should merge into one");

    // readdir inside shared: both content producers fire
    let mut req = readdir_req("shared");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();
    let mut names: Vec<&str> = req.nodes.iter().map(NamedNode::name).collect();
    names.sort_unstable();
    assert_eq!(names, &["a.txt", "b.txt"]);
}

#[test]
fn nested_same_named_dirs_merge_recursively() {
    struct P;
    impl P {
        fn file_a(_: &Self, _ctx: &RouteCtx, _req: &Request) -> Option<NamedNode> { Some(Node::file().named("a.txt")) }

        fn file_b(_: &Self, _ctx: &RouteCtx, _req: &Request) -> Option<NamedNode> { Some(Node::file().named("b.txt")) }
    }

    let tree = RouteTree::builder()
        .dir("level1", |d| d.dir("level2", |d| d.content(P::file_a)))
        .dir("level1", |d| d.dir("level2", |d| d.content(P::file_b)))
        .build();

    let mut req = readdir_req("level1/level2");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();
    let mut names: Vec<&str> = req.nodes.iter().map(NamedNode::name).collect();
    names.sort_unstable();
    assert_eq!(names, &["a.txt", "b.txt"]);
}

#[test]
fn dir_merge_combines_callbacks() {
    struct P;
    impl P {
        fn readdir_a(_: &Self, _ctx: &RouteCtx, req: &mut Request) -> Result<()> {
            req.nodes.add(NamedNode::dir("from-a"));
            Ok(())
        }

        fn readdir_b(_: &Self, _ctx: &RouteCtx, req: &mut Request) -> Result<()> {
            req.nodes.add(NamedNode::dir("from-b"));
            Ok(())
        }
    }

    let tree = RouteTree::builder()
        .dir("merged", |d| d.on_readdir(P::readdir_a))
        .dir("merged", |d| d.on_readdir(P::readdir_b))
        .build();

    let mut req = readdir_req("merged");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();
    let mut names: Vec<&str> = req.nodes.iter().map(NamedNode::name).collect();
    names.sort_unstable();
    assert_eq!(names, &["from-a", "from-b"]);
}

#[rstest]
#[case::readdir("symbols", Op::Readdir, &["Foo"])]
#[case::lookup_hit("symbols", Op::Lookup { name: "Foo".into() }, &["Foo"])]
#[case::lookup_miss("symbols", Op::Lookup { name: "nope".into() }, &[])]
fn rest_subtree_callbacks_fire_on_parent_lookup(#[case] path: &str, #[case] op: Op, #[case] expected_names: &[&str]) {
    struct P;
    impl P {
        fn rest_readdir(_: &Self, ctx: &RouteCtx, req: &mut Request) -> Result<()> {
            assert_eq!(ctx.param("path").is_some(), true);
            req.nodes.add(NamedNode::dir("Foo"));
            Ok(())
        }

        fn rest_lookup(_: &Self, ctx: &RouteCtx, req: &mut Request, name: &str) -> Result<()> {
            assert!(ctx.param("path").is_some(), "rest param should be set");
            if name == "Foo" {
                req.nodes.add(NamedNode::dir("Foo"));
            }
            Ok(())
        }
    }

    let tree = RouteTree::builder()
        .dir("symbols", |d| {
            d.rest("path", |d| d.on_readdir(P::rest_readdir).on_lookup(P::rest_lookup))
        })
        .build();

    let mut req = Request::new(PathBuf::from(path), op);
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    let mut names: Vec<&str> = req.nodes.iter().map(NamedNode::name).collect();
    names.sort_unstable();
    let mut expected: Vec<&str> = expected_names.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);
}
