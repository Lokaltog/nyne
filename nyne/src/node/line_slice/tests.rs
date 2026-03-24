use super::*;
use crate::node::builtins::StaticContent;
use crate::test_support::*;

/// Tests that derive produces a sliced node for a valid slice suffix.
#[test]
fn derive_matches_valid_slice() {
    let base = Arc::new(VirtualNode::file("BLAME.md", StaticContent(b"line1\nline2\nline3\n")));
    let plugin = LineSlice;
    let b = stub_request_context_at("test");
    let result = plugin.derive(&base, "BLAME.md:1-2", &b.ctx()).unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().name(), "BLAME.md:1-2");
}

/// Tests that derive returns None when the base name does not match.
#[test]
fn derive_declines_non_matching_name() {
    let base = Arc::new(VirtualNode::file("LOG.md", StaticContent(b"content")));
    let plugin = LineSlice;
    let b = stub_request_context_at("test");
    let result = plugin.derive(&base, "BLAME.md:1-2", &b.ctx()).unwrap();
    assert!(result.is_none());
}

/// Tests that derive returns None when no slice suffix is present.
#[test]
fn derive_declines_no_slice_suffix() {
    let base = Arc::new(VirtualNode::file("BLAME.md", StaticContent(b"content")));
    let plugin = LineSlice;
    let b = stub_request_context_at("test");
    let result = plugin.derive(&base, "BLAME.md", &b.ctx()).unwrap();
    assert!(result.is_none());
}

/// Tests that sliced content correctly extracts the specified line range.
#[test]
fn sliced_content_extracts_lines() {
    let base = Arc::new(VirtualNode::file("TEST.md", StaticContent(b"aaa\nbbb\nccc\nddd")));
    let plugin = LineSlice;
    let b = stub_request_context_at("test");
    let derived = plugin.derive(&base, "TEST.md:2-3", &b.ctx()).unwrap().unwrap();
    let content = derived.require_readable().unwrap().read(&b.ctx()).unwrap();
    assert_eq!(content, b"bbb\nccc");
}

/// Verifies that derived sliced nodes are hidden from readdir.
#[test]
fn derived_node_is_hidden() {
    use crate::node::Visibility;
    let base = Arc::new(VirtualNode::file("X.md", StaticContent(b"line")));
    let plugin = LineSlice;
    let b = stub_request_context_at("test");
    let derived = plugin.derive(&base, "X.md:1", &b.ctx()).unwrap().unwrap();
    assert_eq!(derived.visibility(), Visibility::Hidden);
}

/// Tests that the derived node is writable when the base node is writable.
#[test]
fn derive_writable_when_base_writable() {
    struct StubWritable;
    impl Writable for StubWritable {
        fn write(&self, _ctx: &RequestContext<'_>, _data: &[u8]) -> Result<WriteOutcome> { Ok(WriteOutcome::Ignored) }
    }

    let base = Arc::new(VirtualNode::file("lines", StaticContent(b"a\nb\nc\n")).with_writable(StubWritable));
    let plugin = LineSlice;
    let b = stub_request_context_at("test");
    let derived = plugin.derive(&base, "lines:2", &b.ctx()).unwrap().unwrap();
    assert!(derived.writable().is_some());
}

/// Tests that the derived node is not writable when the base is read-only.
#[test]
fn derive_not_writable_when_base_readonly() {
    let base = Arc::new(VirtualNode::file("BLAME.md", StaticContent(b"content")));
    let plugin = LineSlice;
    let b = stub_request_context_at("test");
    let derived = plugin.derive(&base, "BLAME.md:1", &b.ctx()).unwrap().unwrap();
    assert!(derived.writable().is_none());
}

/// Tests that splice_lines replaces a line range correctly.
#[test]
fn splice_lines_replaces_range() {
    let current = b"aaa\nbbb\nccc\nddd\n";
    let spec = SliceSpec::Range(2, 3);
    let result = splice_lines(current, &spec, b"XXX\nYYY");
    assert_eq!(result, b"aaa\nXXX\nYYY\nddd\n");
}

/// Tests that splice_lines replaces a single line.
#[test]
fn splice_lines_single() {
    let current = b"aaa\nbbb\nccc\n";
    let spec = SliceSpec::Single(2);
    let result = splice_lines(current, &spec, b"ZZZ");
    assert_eq!(result, b"aaa\nZZZ\nccc\n");
}

/// Tests that splice_lines replaces trailing lines.
#[test]
fn splice_lines_tail() {
    let current = b"aaa\nbbb\nccc\n";
    let spec = SliceSpec::Tail(1);
    let result = splice_lines(current, &spec, b"NEW");
    assert_eq!(result, b"aaa\nbbb\nNEW\n");
}
/// Tests that the derived node is unlinkable when the base is writable.
#[test]
fn derive_unlinkable_when_base_writable() {
    struct StubWritable;
    impl Writable for StubWritable {
        fn write(&self, _ctx: &RequestContext<'_>, _data: &[u8]) -> Result<WriteOutcome> { Ok(WriteOutcome::Ignored) }
    }

    let base = Arc::new(VirtualNode::file("lines", StaticContent(b"a\nb\nc\n")).with_writable(StubWritable));
    let plugin = LineSlice;
    let b = stub_request_context_at("test");
    let derived = plugin.derive(&base, "lines:2", &b.ctx()).unwrap().unwrap();
    assert!(derived.unlinkable().is_some());
}

/// Tests that the derived node is not unlinkable when the base is read-only.
#[test]
fn derive_not_unlinkable_when_base_readonly() {
    let base = Arc::new(VirtualNode::file("BLAME.md", StaticContent(b"content")));
    let plugin = LineSlice;
    let b = stub_request_context_at("test");
    let derived = plugin.derive(&base, "BLAME.md:1", &b.ctx()).unwrap().unwrap();
    assert!(derived.unlinkable().is_none());
}

/// Tests that splicing empty data deletes the targeted line range.
#[test]
fn splice_lines_empty_data_deletes_range() {
    let current = b"aaa\nbbb\nccc\nddd\n";
    let spec = SliceSpec::Range(2, 3);
    let result = splice_lines(current, &spec, b"");
    assert_eq!(result, b"aaa\nddd\n");
}
