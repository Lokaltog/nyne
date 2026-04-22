use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::router::{Chain, NamedNode, Next, Node, Op, Provider, Request};
use nyne::test_support::StubReadable;
use rstest::rstest;

use super::{Visibility, VisibilityProvider};

fn chain(visibility: Visibility) -> Chain {
    let provider = VisibilityProvider {
        policy: Box::new(move |_| Some(visibility)),
    };
    Chain::build(vec![Arc::new(provider) as Arc<dyn Provider>]).unwrap()
}

fn file_with_backing(name: &str) -> NamedNode {
    Node::file()
        .with_readable(StubReadable::empty().with_backing("real"))
        .named(name)
}

fn virtual_file(name: &str) -> NamedNode { Node::file().with_readable(StubReadable::empty()).named(name) }

fn names(req: &Request) -> Vec<&str> {
    let mut v: Vec<&str> = req.nodes.iter().map(NamedNode::name).collect();
    v.sort_unstable();
    v
}

fn dispatch(chain: &Chain, op: Op, nodes: Vec<NamedNode>) -> Request {
    let mut req = Request::new(PathBuf::from("dir"), op);
    for node in nodes {
        req.nodes.add(node);
    }
    chain.dispatch(&mut req).unwrap();
    req
}

#[rstest]
#[case::readdir(Op::Readdir)]
#[case::lookup(Op::Lookup { name: "virtual.rs".into() })]
fn hidden_strips_virtual_nodes(#[case] op: Op) {
    let chain = chain(Visibility::Hidden);
    let req = dispatch(&chain, op, vec![
        file_with_backing("real.rs"),
        virtual_file("virtual.rs"),
    ]);
    assert_eq!(names(&req), &["real.rs"]);
}

#[rstest]
#[case::readdir(Op::Readdir)]
#[case::lookup(Op::Lookup { name: "virtual.rs".into() })]
fn default_keeps_virtual_nodes(#[case] op: Op) {
    let chain = chain(Visibility::Default);
    let req = dispatch(&chain, op, vec![
        file_with_backing("real.rs"),
        virtual_file("virtual.rs"),
    ]);
    assert_eq!(names(&req), &["real.rs", "virtual.rs"]);
}

#[rstest]
#[case::readdir(Op::Readdir)]
#[case::lookup(Op::Lookup { name: "virtual.rs".into() })]
fn force_keeps_all_nodes(#[case] op: Op) {
    let chain = chain(Visibility::Force);
    let req = dispatch(&chain, op, vec![
        file_with_backing("real.rs"),
        virtual_file("virtual.rs"),
    ]);
    assert_eq!(names(&req), &["real.rs", "virtual.rs"]);
}

/// Inner provider that overwrites the visibility state — simulates the cache
/// middleware restoring a snapshot from a non-hidden process on cache hit.
struct StateOverwriter;
nyne::define_provider!(StateOverwriter, "overwriter");
impl Provider for StateOverwriter {
    fn accept(&self, req: &mut Request, next: &Next) -> Result<()> {
        // Simulate cache restore_state overwriting Hidden → Default.
        req.set_state(Visibility::Default);
        next.run(req)
    }
}

/// Regression: the visibility post-filter must use its own computed value,
/// not re-read state that inner middleware (cache) may have overwritten.
#[rstest]
#[case::readdir(Op::Readdir)]
#[case::lookup(Op::Lookup { name: "virtual.rs".into() })]
fn hidden_post_filter_survives_state_overwrite(#[case] op: Op) {
    let visibility = VisibilityProvider {
        policy: Box::new(move |_| Some(Visibility::Hidden)),
    };
    let overwriter = StateOverwriter;
    let chain = Chain::build(vec![
        Arc::new(visibility) as Arc<dyn Provider>,
        Arc::new(overwriter) as Arc<dyn Provider>,
    ])
    .unwrap();
    let req = dispatch(&chain, op, vec![
        file_with_backing("real.rs"),
        virtual_file("virtual.rs"),
    ]);
    // Virtual nodes must be stripped despite the inner provider overwriting
    // Hidden → Default on the state map.
    assert_eq!(names(&req), &["real.rs"]);
}
