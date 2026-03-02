use super::*;
use crate::dispatch::cache::CachedNode;
use crate::node::VirtualNode;
use crate::node::builtins::StaticContent;
use crate::node::plugin::NodePlugin;
use crate::provider::ProviderId;
use crate::test_support::stub_request_context_at;

/// Mock plugin that derives a node when the name has a `:echo` suffix.
struct EchoPlugin;

impl NodePlugin for EchoPlugin {
    fn derive(
        &self,
        _base: &Arc<VirtualNode>,
        name: &str,
        _ctx: &RequestContext<'_>,
    ) -> color_eyre::eyre::Result<Option<VirtualNode>> {
        if let Some(stripped) = name.strip_suffix(":echo") {
            Ok(Some(VirtualNode::file(stripped, StaticContent(b"echoed"))))
        } else {
            Ok(None)
        }
    }
}

fn insert_virtual(dir: &mut DirState, name: &str, node: VirtualNode) {
    dir.insert(name.to_owned(), CachedNode {
        inode: 1,
        kind: CachedNodeKind::Virtual {
            node: Arc::new(node),
            provider_id: ProviderId::new("test"),
        },
        source: NodeSource::Children,
        generation: dir.resolve_generation(),
    });
}

#[test]
fn derive_from_plugins_returns_match() {
    let mut dir = DirState::new();
    dir.begin_resolve();
    insert_virtual(
        &mut dir,
        "BLAME.md",
        VirtualNode::file("BLAME.md", StaticContent(b"")).plugin(EchoPlugin),
    );

    let stub = stub_request_context_at("src/foo.rs@/symbols");
    let result = derive_from_plugins(&dir, "BLAME.md:echo", &stub.ctx()).unwrap();

    let entry = result.expect("should derive a node");
    assert_eq!(entry.name, "BLAME.md:echo");
    assert!(matches!(entry.source, NodeSource::Derived));
}

#[test]
fn derive_from_plugins_returns_none_on_no_match() {
    let mut dir = DirState::new();
    dir.begin_resolve();
    insert_virtual(
        &mut dir,
        "BLAME.md",
        VirtualNode::file("BLAME.md", StaticContent(b"")).plugin(EchoPlugin),
    );

    let stub = stub_request_context_at("src/foo.rs@/symbols");
    let result = derive_from_plugins(&dir, "no-match", &stub.ctx()).unwrap();
    assert!(result.is_none());
}

#[test]
fn derive_from_plugins_skips_non_plugin_nodes() {
    let mut dir = DirState::new();
    dir.begin_resolve();
    // Node without plugins — should be skipped.
    insert_virtual(&mut dir, "plain.md", VirtualNode::file("plain.md", StaticContent(b"")));

    let stub = stub_request_context_at("src/foo.rs@/symbols");
    let result = derive_from_plugins(&dir, "plain.md:echo", &stub.ctx()).unwrap();
    assert!(result.is_none());
}

#[test]
fn derive_from_plugins_skips_real_entries() {
    let mut dir = DirState::new();
    dir.begin_resolve();
    dir.insert("real.txt".to_owned(), CachedNode {
        inode: 1,
        kind: CachedNodeKind::Real {
            file_type: FileKind::File,
        },
        source: NodeSource::Children,
        generation: dir.resolve_generation(),
    });

    let stub = stub_request_context_at("src");
    let result = derive_from_plugins(&dir, "real.txt:echo", &stub.ctx()).unwrap();
    assert!(result.is_none());
}
