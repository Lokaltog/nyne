use rstest::rstest;

use super::*;
use crate::test_support::{StubReadable, StubWritable};

#[test]
fn bitor_combines_flags() {
    let perms = Permissions::READ | Permissions::WRITE;
    assert!(perms.contains(Permissions::READ));
    assert!(perms.contains(Permissions::WRITE));
    assert!(!perms.contains(Permissions::EXECUTE));
}

#[test]
fn bitand_intersects_flags() {
    let rw = Permissions::READ | Permissions::WRITE;
    let rx = Permissions::READ | Permissions::EXECUTE;
    assert!((rw & rx).contains(Permissions::READ));
    assert!(!(rw & rx).contains(Permissions::WRITE));
    assert!(!(rw & rx).contains(Permissions::EXECUTE));
}

#[test]
fn not_inverts_within_3_bits() {
    let inverted = !Permissions::READ;
    assert!(!inverted.contains(Permissions::READ));
    assert!(inverted.contains(Permissions::WRITE));
    assert!(inverted.contains(Permissions::EXECUTE));
}

#[test]
fn none_is_empty() {
    assert!(Permissions::NONE.is_empty());
    assert!(!Permissions::READ.is_empty());
}

#[test]
fn all_contains_every_flag() {
    assert!(Permissions::ALL.contains(Permissions::READ));
    assert!(Permissions::ALL.contains(Permissions::WRITE));
    assert!(Permissions::ALL.contains(Permissions::EXECUTE));
}

#[test]
fn from_bits_round_trips() {
    let perms = Permissions::READ | Permissions::EXECUTE;
    assert_eq!(Permissions::from_bits_masked(perms.bits()), perms);
}

#[test]
#[should_panic(expected = "out-of-range")]
fn from_bits_rejects_out_of_range() { let _ = Permissions::from_bits_masked(0xFF); }

#[rstest]
#[case::all("rwx", Permissions::ALL)]
#[case::none("---", Permissions::NONE)]
#[case::read_execute("r-x", Permissions::READ | Permissions::EXECUTE)]
#[case::write_only("-w-", Permissions::WRITE)]
fn display_formats_rwx(#[case] expected: &str, #[case] perms: Permissions) {
    assert_eq!(perms.to_string(), expected);
}

#[test]
fn bitor_assign_accumulates() {
    let mut perms = Permissions::READ;
    perms |= Permissions::WRITE;
    assert!(perms.contains(Permissions::READ));
    assert!(perms.contains(Permissions::WRITE));
}

/// Auto-derived permissions from node kind and capabilities.
#[rstest]
#[case::file_no_caps(Node::file(), Permissions::NONE)]
#[case::file_readable(Node::file().with_readable(StubReadable::new("")), Permissions::READ)]
#[case::file_writable(Node::file().with_writable(StubWritable), Permissions::WRITE)]
#[case::file_read_write(
    Node::file().with_readable(StubReadable::new("")).with_writable(StubWritable),
    Permissions::READ | Permissions::WRITE,
)]
#[case::dir_default(Node::dir(), Permissions::READ | Permissions::EXECUTE)]
#[case::dir_writable(Node::dir().with_writable(StubWritable), Permissions::ALL)]
#[case::symlink(Node::symlink("/target"), Permissions::ALL)]
#[case::explicit_override(
    Node::file().with_readable(StubReadable::new("")).with_permissions(Permissions::ALL),
    Permissions::ALL,
)]
fn auto_derived_permissions(#[case] node: Node, #[case] expected: Permissions) {
    assert_eq!(node.permissions(), expected);
}

/// Merge: first-writer-wins for explicit permissions.
#[rstest]
#[case::first_wins(
    Node::file().with_permissions(Permissions::READ),
    Node::file().with_permissions(Permissions::WRITE),
    Permissions::READ,
)]
#[case::takes_other_when_self_has_none(
    Node::file(),
    Node::file().with_permissions(Permissions::WRITE),
    Permissions::WRITE,
)]
fn merge_permissions(#[case] mut target: Node, #[case] source: Node, #[case] expected: Permissions) {
    target.merge_capabilities_from(source);
    assert_eq!(target.permissions(), expected);
}

#[test]
fn cache_policy_default_variant() {
    assert_eq!(CachePolicy::default(), CachePolicy::Default);
}

#[test]
fn node_cache_policy_defaults_to_default_variant() {
    assert_eq!(Node::file().cache_policy(), CachePolicy::Default);
}

#[rstest]
#[case::no_cache(CachePolicy::NoCache)]
#[case::ttl(CachePolicy::Ttl(Duration::from_secs(60)))]
#[case::default(CachePolicy::Default)]
fn node_with_cache_policy_round_trips(#[case] policy: CachePolicy) {
    assert_eq!(Node::file().with_cache_policy(policy).cache_policy(), policy);
}

#[rstest]
#[case::first_writer_wins_no_cache_over_ttl(
    Node::file().with_cache_policy(CachePolicy::NoCache),
    Node::file().with_cache_policy(CachePolicy::Ttl(Duration::from_secs(30))),
    CachePolicy::NoCache,
)]
#[case::default_target_takes_other(
    Node::file(),
    Node::file().with_cache_policy(CachePolicy::Ttl(Duration::from_secs(30))),
    CachePolicy::Ttl(Duration::from_secs(30)),
)]
#[case::default_source_leaves_target_default(
    Node::file().with_cache_policy(CachePolicy::Ttl(Duration::from_secs(5))),
    Node::file(),
    CachePolicy::Ttl(Duration::from_secs(5)),
)]
fn merge_cache_policy(#[case] mut target: Node, #[case] source: Node, #[case] expected: CachePolicy) {
    target.merge_capabilities_from(source);
    assert_eq!(target.cache_policy(), expected);
}

#[test]
fn readable_size_defaults_to_none() {
    assert!(StubReadable::new("hello").size().is_none());
}

struct StubLifecycle;
impl Lifecycle for StubLifecycle {}

struct StubAttributable;
impl Attributable for StubAttributable {
    fn get(&self, _key: &str) -> Option<Vec<u8>> { None }

    fn set(&self, _key: &str, _value: &[u8]) -> Result<()> { Ok(()) }

    fn list(&self) -> Vec<String> { Vec::new() }
}

#[rstest]
#[case::lifecycle(Node::file().with_lifecycle(StubLifecycle))]
#[case::attributable(Node::file().with_attributable(StubAttributable))]
#[case::readable(Node::file().with_readable(StubReadable::new("")))]
#[case::writable(Node::file().with_writable(StubWritable))]
fn capability_slot_attach(#[case] node: Node) {
    assert!(
        node.readable().is_some()
            || node.writable().is_some()
            || node.lifecycle().is_some()
            || node.attributable().is_some()
    );
}

#[rstest]
#[case::lifecycle(Node::file(), Node::file().with_lifecycle(StubLifecycle))]
#[case::attributable(Node::file(), Node::file().with_attributable(StubAttributable))]
fn capability_slot_merges_when_empty(#[case] mut target: Node, #[case] source: Node) {
    target.merge_capabilities_from(source);
    assert!(target.lifecycle().is_some() || target.attributable().is_some());
}
#[rstest]
#[case::static_closure(
    Box::new(LazyReadable::new(|_ctx: &ReadContext<'_>| Ok(b"hello from closure".to_vec()))) as Box<dyn Readable>,
    b"hello from closure".as_slice(),
)]
#[case::captured_state({
    let prefix = String::from("captured");
    Box::new(LazyReadable::new(move |_ctx: &ReadContext<'_>| Ok(format!("{prefix}-value").into_bytes()))) as Box<dyn Readable>
}, b"captured-value".as_slice())]
fn lazy_readable_returns_expected(#[case] readable: Box<dyn Readable>, #[case] expected: &[u8]) {
    let fs = crate::router::fs::mem::MemFs::new();
    let ctx = ReadContext {
        path: Path::new("/test"),
        fs: &fs,
    };
    assert_eq!(readable.read(&ctx).unwrap(), expected);
}

#[test]
fn lazy_readable_propagates_errors() {
    let fs = crate::router::fs::mem::MemFs::new();
    let ctx = ReadContext {
        path: Path::new("/test"),
        fs: &fs,
    };
    let readable = LazyReadable::new(|_ctx| color_eyre::eyre::bail!("intentional failure"));
    let err = readable.read(&ctx).unwrap_err();
    assert!(err.to_string().contains("intentional failure"));
}
#[test]
fn named_node_into_parts_roundtrip() {
    let node = Node::file().with_readable(StubReadable::new("content"));
    let named = node.named("test.txt");

    assert_eq!(named.name(), "test.txt");
    assert!(named.readable().is_some());

    let (name, inner) = named.into_parts();
    assert_eq!(name, "test.txt");
    assert!(inner.readable().is_some());

    // Reconstruct
    let rebuilt = NamedNode::new(name, inner);
    assert_eq!(rebuilt.name(), "test.txt");
    assert!(rebuilt.readable().is_some());
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "cannot merge capabilities across different node kinds")]
fn merge_rejects_kind_mismatch() {
    let mut file = Node::file().with_readable(StubReadable::new("a"));
    let dir = Node::dir();
    file.merge_capabilities_from(dir);
}
