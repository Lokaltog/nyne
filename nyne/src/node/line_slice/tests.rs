use super::*;
use crate::node::builtins::StaticContent;
use crate::test_support::*;

#[test]
fn derive_matches_valid_slice() {
    let base = Arc::new(VirtualNode::file("BLAME.md", StaticContent(b"line1\nline2\nline3\n")));
    let plugin = LineSlice;
    let b = stub_request_context_at("test");
    let result = plugin.derive(&base, "BLAME.md:1-2", &b.ctx()).unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().name(), "BLAME.md:1-2");
}

#[test]
fn derive_declines_non_matching_name() {
    let base = Arc::new(VirtualNode::file("LOG.md", StaticContent(b"content")));
    let plugin = LineSlice;
    let b = stub_request_context_at("test");
    let result = plugin.derive(&base, "BLAME.md:1-2", &b.ctx()).unwrap();
    assert!(result.is_none());
}

#[test]
fn derive_declines_no_slice_suffix() {
    let base = Arc::new(VirtualNode::file("BLAME.md", StaticContent(b"content")));
    let plugin = LineSlice;
    let b = stub_request_context_at("test");
    let result = plugin.derive(&base, "BLAME.md", &b.ctx()).unwrap();
    assert!(result.is_none());
}

#[test]
fn sliced_content_extracts_lines() {
    let base = Arc::new(VirtualNode::file("TEST.md", StaticContent(b"aaa\nbbb\nccc\nddd")));
    let plugin = LineSlice;
    let b = stub_request_context_at("test");
    let derived = plugin.derive(&base, "TEST.md:2-3", &b.ctx()).unwrap().unwrap();
    let content = derived.require_readable().unwrap().read(&b.ctx()).unwrap();
    assert_eq!(content, b"bbb\nccc");
}

#[test]
fn derived_node_is_hidden() {
    use crate::node::Visibility;
    let base = Arc::new(VirtualNode::file("X.md", StaticContent(b"line")));
    let plugin = LineSlice;
    let b = stub_request_context_at("test");
    let derived = plugin.derive(&base, "X.md:1", &b.ctx()).unwrap().unwrap();
    assert_eq!(derived.visibility(), Visibility::Hidden);
}

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

#[test]
fn derive_not_writable_when_base_readonly() {
    let base = Arc::new(VirtualNode::file("BLAME.md", StaticContent(b"content")));
    let plugin = LineSlice;
    let b = stub_request_context_at("test");
    let derived = plugin.derive(&base, "BLAME.md:1", &b.ctx()).unwrap().unwrap();
    assert!(derived.writable().is_none());
}

#[test]
fn splice_lines_replaces_range() {
    let current = b"aaa\nbbb\nccc\nddd\n";
    let spec = SliceSpec::Range(2, 3);
    let result = splice_lines(current, &spec, b"XXX\nYYY");
    assert_eq!(result, b"aaa\nXXX\nYYY\nddd\n");
}

#[test]
fn splice_lines_single() {
    let current = b"aaa\nbbb\nccc\n";
    let spec = SliceSpec::Single(2);
    let result = splice_lines(current, &spec, b"ZZZ");
    assert_eq!(result, b"aaa\nZZZ\nccc\n");
}

#[test]
fn splice_lines_tail() {
    let current = b"aaa\nbbb\nccc\n";
    let spec = SliceSpec::Tail(1);
    let result = splice_lines(current, &spec, b"NEW");
    assert_eq!(result, b"aaa\nbbb\nNEW\n");
}
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

#[test]
fn derive_not_unlinkable_when_base_readonly() {
    let base = Arc::new(VirtualNode::file("BLAME.md", StaticContent(b"content")));
    let plugin = LineSlice;
    let b = stub_request_context_at("test");
    let derived = plugin.derive(&base, "BLAME.md:1", &b.ctx()).unwrap().unwrap();
    assert!(derived.unlinkable().is_none());
}

#[test]
fn splice_lines_empty_data_deletes_range() {
    let current = b"aaa\nbbb\nccc\nddd\n";
    let spec = SliceSpec::Range(2, 3);
    let result = splice_lines(current, &spec, b"");
    assert_eq!(result, b"aaa\nddd\n");
}
