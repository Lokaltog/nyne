use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rstest::rstest;

use crate::router::tree::RouteTree;
use crate::router::{NamedNode, Next, Node, Op, Request, RouteExtension};
use crate::test_support::{StubReadable, readdir_req};

struct P;

fn names(req: &Request) -> Vec<&str> {
    let mut v: Vec<&str> = req.nodes.iter().map(NamedNode::name).collect();
    v.sort_unstable();
    v
}

#[rstest]
#[case::default(|_ext: &mut RouteExtension| {}, true)]
#[case::with_readdir(|ext: &mut RouteExtension| { ext.on_readdir(|_, _| Ok(())); }, false)]
#[case::with_lookup(|ext: &mut RouteExtension| { ext.on_lookup(|_, _, _| Ok(())); }, false)]
#[case::with_content(|ext: &mut RouteExtension| { ext.content(|_, _| None); }, false)]
#[case::with_handler(|ext: &mut RouteExtension| { ext.handler(|_, _, _| Ok(())); }, false)]
#[case::with_dir(|ext: &mut RouteExtension| { ext.dir("d", |_| {}); }, false)]
#[case::with_capture(|ext: &mut RouteExtension| { ext.capture("p", |_| {}); }, false)]
#[case::with_rest(|ext: &mut RouteExtension| { ext.rest("p", |_| {}); }, false)]
fn is_empty(#[case] setup: impl FnOnce(&mut RouteExtension), #[case] expected: bool) {
    let mut ext = RouteExtension::new();
    setup(&mut ext);
    assert_eq!(ext.is_empty(), expected);
}

#[rstest]
#[case::readdir(Op::Readdir, &["from-ext"])]
#[case::lookup_hit(Op::Lookup { name: "from-ext".into() }, &["from-ext"])]
#[case::lookup_miss(Op::Lookup { name: "other".into() }, &[])]
fn on_readdir_fires_through_apply(#[case] op: Op, #[case] expected: &[&str]) {
    let mut ext = RouteExtension::new();
    ext.on_readdir(|_ctx, req| {
        req.nodes.add(NamedNode::dir("from-ext"));
        Ok(())
    });

    let tree: RouteTree<P> = RouteTree::builder().apply(&ext).build();
    let mut req = Request::new(PathBuf::from(""), op);
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    assert_eq!(names(&req), expected);
}

#[rstest]
#[case::lookup_hit(Op::Lookup { name: "from-ext".into() }, &["from-ext"])]
#[case::lookup_miss(Op::Lookup { name: "other".into() }, &[])]
#[case::not_on_readdir(Op::Readdir, &[])]
fn on_lookup_fires_through_apply(#[case] op: Op, #[case] expected: &[&str]) {
    let mut ext = RouteExtension::new();
    ext.on_lookup(|_ctx, req, name| {
        if name == "from-ext" {
            req.nodes.add(NamedNode::dir("from-ext"));
        }
        Ok(())
    });

    let tree: RouteTree<P> = RouteTree::builder().apply(&ext).build();
    let mut req = Request::new(PathBuf::from(""), op);
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    assert_eq!(names(&req), expected);
}

#[rstest]
#[case::readdir(Op::Readdir, &["ext.txt"])]
#[case::lookup_hit(Op::Lookup { name: "ext.txt".into() }, &["ext.txt"])]
#[case::lookup_miss(Op::Lookup { name: "other".into() }, &[])]
fn content_produces_nodes(#[case] op: Op, #[case] expected: &[&str]) {
    let mut ext = RouteExtension::new();
    ext.content(|_ctx, _req| Some(Node::file().with_readable(StubReadable::new("hello")).named("ext.txt")));

    let tree: RouteTree<P> = RouteTree::builder().apply(&ext).build();
    let mut req = Request::new(PathBuf::from(""), op);
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    assert_eq!(names(&req), expected);
}

#[rstest]
#[case::readdir(Op::Readdir, &["always.txt"])]
#[case::lookup_hit(Op::Lookup { name: "always.txt".into() }, &["always.txt"])]
#[case::lookup_miss(Op::Lookup { name: "other".into() }, &[])]
fn content_always_produces_nodes(#[case] op: Op, #[case] expected: &[&str]) {
    let mut ext = RouteExtension::new();
    ext.content_always(|_ctx, _req| {
        Node::file()
            .with_readable(StubReadable::new("always"))
            .named("always.txt")
    });

    let tree: RouteTree<P> = RouteTree::builder().apply(&ext).build();
    let mut req = Request::new(PathBuf::from(""), op);
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    assert_eq!(names(&req), expected);
}

#[test]
fn multiple_readdir_callbacks_all_fire() {
    let mut ext = RouteExtension::new();
    ext.on_readdir(|_ctx, req| {
        req.nodes.add(NamedNode::dir("first"));
        Ok(())
    });
    ext.on_readdir(|_ctx, req| {
        req.nodes.add(NamedNode::dir("second"));
        Ok(())
    });

    let tree: RouteTree<P> = RouteTree::builder().apply(&ext).build();
    let mut req = readdir_req("");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    assert_eq!(names(&req), &["first", "second"]);
}

#[test]
fn multiple_content_all_emit() {
    let mut ext = RouteExtension::new();
    ext.content(|_, _| Some(Node::file().named("a.txt")));
    ext.content(|_, _| Some(Node::file().named("b.txt")));

    let tree: RouteTree<P> = RouteTree::builder().apply(&ext).build();
    let mut req = readdir_req("");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    assert_eq!(names(&req), &["a.txt", "b.txt"]);
}

#[test]
fn handler_replaces_dispatch() {
    let mut ext = RouteExtension::new();
    ext.handler(|_ctx, req, next| {
        next.run(req)?;
        req.nodes.add(NamedNode::dir("from-handler"));
        Ok(())
    });

    let tree: RouteTree<P> = RouteTree::builder().apply(&ext).build();
    let mut req = readdir_req("");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    assert!(req.nodes.find("from-handler").is_some());
}

#[test]
fn handler_is_singular_last_wins() {
    let mut ext = RouteExtension::new();
    ext.handler(|_ctx, req, _next| {
        req.nodes.add(NamedNode::dir("first"));
        Ok(())
    });
    ext.handler(|_ctx, req, _next| {
        req.nodes.add(NamedNode::dir("second"));
        Ok(())
    });

    let tree: RouteTree<P> = RouteTree::builder().apply(&ext).build();
    let mut req = readdir_req("");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    assert!(req.nodes.find("first").is_none(), "first handler should be replaced");
    assert!(req.nodes.find("second").is_some(), "second handler should win");
}

#[test]
fn dir_creates_subdirectory() {
    let mut ext = RouteExtension::new();
    ext.dir("sub", |ext| {
        ext.content(|_, _| Some(Node::file().named("inner.txt")));
    });

    let tree: RouteTree<P> = RouteTree::builder().apply(&ext).build();

    let mut req = readdir_req("");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();
    assert!(req.nodes.find("sub").is_some());

    let mut req = readdir_req("sub");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();
    assert_eq!(names(&req), &["inner.txt"]);
}

#[test]
fn nested_dirs_compose() {
    let mut ext = RouteExtension::new();
    ext.dir("a", |ext| {
        ext.dir("b", |ext| {
            ext.content(|_, _| Some(Node::file().named("deep.txt")));
        });
    });

    let tree: RouteTree<P> = RouteTree::builder().apply(&ext).build();

    let mut req = readdir_req("a/b");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();
    assert_eq!(names(&req), &["deep.txt"]);
}

#[test]
fn capture_binds_param() {
    let mut ext = RouteExtension::new();
    ext.capture("name", |ext| {
        ext.content(|ctx, _req| {
            Some(
                Node::file()
                    .with_readable(StubReadable::new(ctx.param("name").unwrap_or("unknown")))
                    .named("body.rs"),
            )
        });
    });

    let tree: RouteTree<P> = RouteTree::builder().apply(&ext).build();

    let mut req = readdir_req("hello");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();
    assert_eq!(names(&req), &["body.rs"]);
}

#[test]
fn rest_captures_remaining() {
    let mut ext = RouteExtension::new();
    ext.rest("path", |ext| {
        ext.on_readdir(|ctx, req| {
            req.nodes
                .add(NamedNode::dir(format!("got-{}", ctx.param("path").unwrap_or(""))));
            Ok(())
        });
    });

    let tree: RouteTree<P> = RouteTree::builder().apply(&ext).build();

    let mut req = readdir_req("Foo/bar");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();
    assert_eq!(names(&req), &["got-Foo/bar"]);
}

#[test]
fn extension_coexists_with_typed_content() {
    let mut ext = RouteExtension::new();
    ext.content(|_, _| Some(Node::file().named("from-ext.txt")));

    let tree: RouteTree<P> = RouteTree::builder()
        .content(|_: &P, _, _| Some(Node::file().named("from-provider.txt")))
        .apply(&ext)
        .build();

    let mut req = readdir_req("");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();
    assert_eq!(names(&req), &["from-ext.txt", "from-provider.txt"]);
}

#[test]
fn two_extensions_compose() {
    let mut ext_a = RouteExtension::new();
    ext_a.content(|_, _| Some(Node::file().named("a.txt")));

    let mut ext_b = RouteExtension::new();
    ext_b.content(|_, _| Some(Node::file().named("b.txt")));

    let tree: RouteTree<P> = RouteTree::builder().apply(&ext_a).apply(&ext_b).build();

    let mut req = readdir_req("");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();
    assert_eq!(names(&req), &["a.txt", "b.txt"]);
}

#[test]
fn scoped_merges_entries() {
    let mut ext = RouteExtension::new();
    ext.scoped("test-plugin", |ext| {
        ext.content(|_, _| Some(Node::file().named("scoped.txt")));
        ext.on_readdir(|_ctx, req| {
            req.nodes.add(NamedNode::dir("scoped-dir"));
            Ok(())
        });
    });

    assert!(!ext.is_empty());

    let tree: RouteTree<P> = RouteTree::builder().apply(&ext).build();
    let mut req = readdir_req("");
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();
    assert_eq!(names(&req), &["scoped-dir", "scoped.txt"]);
}

#[test]
fn dir_with_nested_capture() {
    let fired = Arc::new(AtomicBool::new(false));
    let fired_clone = Arc::clone(&fired);

    let mut ext = RouteExtension::new();
    ext.dir("rename", |ext| {
        ext.capture("target", |ext| {
            ext.handler(move |ctx, req, _next| {
                req.nodes.add(
                    Node::file()
                        .with_readable(StubReadable::new(ctx.param("target").unwrap_or("none")))
                        .named("preview.diff"),
                );
                fired_clone.store(true, Ordering::Relaxed);
                Ok(())
            });
        });
    });

    let tree: RouteTree<P> = RouteTree::builder().apply(&ext).build();

    let mut req = Request::new(PathBuf::from("rename/foo"), Op::Readdir);
    tree.dispatch(&P, &mut req, &Next::empty()).unwrap();

    assert!(fired.load(Ordering::Relaxed), "handler should have fired");
    assert_eq!(names(&req), &["preview.diff"]);
}
