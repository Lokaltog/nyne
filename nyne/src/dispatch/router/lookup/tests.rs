use rstest::rstest;

use super::*;
use crate::dispatch::cache::CachedNode;
use crate::dispatch::path_filter::PathFilter;
use crate::dispatch::registry::ProviderRegistry;
use crate::node::VirtualNode;
use crate::node::builtins::StaticContent;
use crate::node::plugin::NodePlugin;
use crate::provider::ProviderId;
use crate::test_support::{StubEvents, stub_request_context_at};
use crate::types::real_fs::OsFs;

/// Mock plugin that derives a node when the name has a `:echo` suffix.
struct EchoPlugin;

/// [`NodePlugin`] implementation that derives nodes when the name has a `:echo` suffix.
impl NodePlugin for EchoPlugin {
    /// Derive a node if the name ends with `:echo`, stripping the suffix.
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

/// Insert a virtual node into a `DirState` for testing.
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

/// Tests that `derive_from_plugins` returns a derived node when a plugin matches.
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

/// Tests that `derive_from_plugins` returns `None` when no plugin matches.
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

/// Tests that nodes without plugins are skipped during derivation.
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

/// Tests that real filesystem entries are skipped during plugin derivation.
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

/// Build a [`Router`] backed by a real temp directory for filesystem tests.
fn router_with_real_fs(dir: &std::path::Path) -> Router {
    Router::new(
        Arc::new(ProviderRegistry::empty()),
        Arc::new(OsFs::new(dir.to_owned())),
        Arc::new(StubEvents),
        PathFilter::build(dir, None),
    )
}

/// Resolve the [`FileKind`] cached for an inode after `lookup_real`.
fn resolved_file_kind(router: &Router, inode: u64) -> FileKind {
    match router.resolve_inode(inode).expect("inode should resolve") {
        ResolvedInode::Real { file_type, .. } => file_type,
        ResolvedInode::Virtual { .. } => panic!("expected Real inode"),
    }
}

/// `lookup_real` must report symlinks as `FileKind::Symlink`, not the
/// target's kind. Regression test for symlink-to-directory misclassification
/// that broke bun/node_modules resolution inside the sandbox.
#[rstest]
#[case::symlink_to_dir("link_to_dir", FileKind::Symlink)]
#[case::symlink_to_file("link_to_file", FileKind::Symlink)]
#[case::real_dir("real_dir", FileKind::Directory)]
#[case::real_file("real_file", FileKind::File)]
fn lookup_real_reports_correct_file_kind(#[case] name: &str, #[case] expected: FileKind) {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("real_dir")).unwrap();
    std::fs::write(tmp.path().join("real_file"), b"content").unwrap();
    std::os::unix::fs::symlink("real_dir", tmp.path().join("link_to_dir")).unwrap();
    std::os::unix::fs::symlink("real_file", tmp.path().join("link_to_file")).unwrap();

    let router = router_with_real_fs(tmp.path());
    let ctx = stub_request_context_at("");

    assert_eq!(
        resolved_file_kind(
            &router,
            router
                .lookup_real(name, &ctx.ctx())
                .unwrap()
                .expect("entry should be found"),
        ),
        expected,
    );
}
