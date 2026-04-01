use std::io::Write as _;
use std::sync::Arc;

use rstest::rstest;

use super::*;

/// Create a temp file with the given content and return a read-only `File` handle.
fn temp_file(content: &[u8]) -> File {
    let mut f = tempfile::tempfile().unwrap();
    f.write_all(content).unwrap();
    f
}

#[rstest]
#[case::overwrite_at_zero(b"hello", 0, b"HELLO", b"HELLO")]
#[case::append_at_end(b"hello", 5, b" world", b"hello world")]
#[case::beyond_end_zero_fills(b"abc", 5, b"xy", b"abc\0\0xy")]
fn buffered_write_at_offset(
    #[case] initial: &[u8],
    #[case] offset: u64,
    #[case] write_data: &[u8],
    #[case] expected: &[u8],
) {
    let table = HandleTable::new();
    let fh = table.open(1, Arc::from(initial), 0);

    let outcome = table.write(fh, offset, write_data).unwrap();
    let WriteOutcome::Buffered(n) = outcome else {
        panic!("expected Buffered, got {outcome:?}");
    };
    assert_eq!(n as usize, write_data.len());

    let data = table.read(fh, 0, 256).unwrap();
    assert_eq!(data, expected);
}

// Direct (fd-backed) handle: write → read back

#[rstest]
#[case::append_populates_from_fd(
    b"original content",
    "original content".len() as u64,
    b" appended",
    b"original content appended",
)]
#[case::overwrite_preserves_surrounding(b"hello world", 6, b"WORLD", b"hello WORLD")]
#[case::write_at_zero_on_empty(b"", 0, b"new content", b"new content")]
fn direct_handle_write(
    #[case] file_content: &[u8],
    #[case] offset: u64,
    #[case] write_data: &[u8],
    #[case] expected: &[u8],
) {
    let table = HandleTable::new();
    let fd = temp_file(file_content);
    let fh = table.open_direct(1, fd, 0);

    table.write(fh, offset, write_data).unwrap();

    let data = table.read(fh, 0, 256).unwrap();
    assert_eq!(data, expected);
}

/// Direct handle read before any write comes from pread on the backing fd.
#[test]
fn direct_handle_read_before_write_uses_fd() {
    let table = HandleTable::new();
    let original = b"original content";
    let fd = temp_file(original);
    let fh = table.open_direct(1, fd, 0);

    let data = table.read(fh, 0, 256).unwrap();
    assert_eq!(data, original);
}

/// A direct handle is clean until written to.
#[test]
fn direct_handle_marks_dirty_after_write() {
    let table = HandleTable::new();
    let fd = temp_file(b"data");
    let fh = table.open_direct(1, fd, 0);

    assert!(table.dirty_snapshot(fh).is_none());

    table.write(fh, 4, b"!").unwrap();

    let snapshot = table.dirty_snapshot(fh).unwrap();
    assert_eq!(snapshot.data, b"data!");
    assert_eq!(snapshot.generation, 1);
}

// O_APPEND: stale kernel offsets are clamped to buffer length

/// `O_APPEND` clamps stale kernel offsets to the buffer length (buffered).
#[test]
fn append_clamps_stale_offset_buffered() {
    let table = HandleTable::new();
    let fh = table.open(1, Arc::from(b"line1\nline2\n".as_slice()), libc::O_APPEND);

    table.write(fh, 100, b"line3\n").unwrap();

    let data = table.read(fh, 0, 256).unwrap();
    assert_eq!(data, b"line1\nline2\nline3\n");
}

/// `O_APPEND` clamps stale kernel offsets to the buffer length (direct fd).
#[test]
fn append_clamps_stale_offset_direct() {
    let table = HandleTable::new();
    let fd = temp_file(b"hello");
    let fh = table.open_direct(1, fd, libc::O_APPEND);

    table.write(fh, 1000, b" world").unwrap();

    let data = table.read(fh, 0, 256).unwrap();
    assert_eq!(data, b"hello world");
}

// O_TRUNC open: truncation + direct handles

/// `open_direct` with `O_TRUNC` followed by truncate + write must NOT
/// repopulate the buffer from the backing fd.
#[test]
fn direct_handle_truncate_then_write_does_not_repopulate() {
    let table = HandleTable::new();
    let original = b"original long content that should disappear after truncation";
    let fd = temp_file(original);
    let fh = table.open_direct(1, fd, libc::O_TRUNC);

    // Kernel sends setattr(size=0) for O_TRUNC opens.
    table.truncate(fh, 0);

    // Write shorter replacement content.
    let replacement = b"short";
    table.write(fh, 0, replacement).unwrap();

    // Buffer must contain ONLY the replacement — no old tail bytes.
    let data = table.read(fh, 0, 256).unwrap();
    assert_eq!(data, replacement, "truncated direct handle leaked old file content");
}

// Dirty generation tracking

/// Dirty generation prevents a concurrent write from being silently lost.
///
/// Sequence: `dirty_snapshot` → write → `clear_dirty(old_gen)`
/// The `clear_dirty` must NOT erase the new write's dirty state.
#[test]
fn clear_dirty_respects_generation() {
    let table = HandleTable::new();
    let fh = table.open(1, Arc::from(b"initial".as_slice()), 0);

    table.write(fh, 0, b"first").unwrap();

    // Snapshot captures generation=1.
    let snap1 = table.dirty_snapshot(fh).unwrap();
    assert_eq!(snap1.generation, 1);

    // Concurrent write bumps generation to 2.
    table.write(fh, 0, b"second").unwrap();

    // clear_dirty with stale generation=1 must NOT clear — generation is now 2.
    table.clear_dirty(fh, snap1.generation);

    // Handle must still be dirty.
    let snap2 = table.dirty_snapshot(fh).unwrap();
    assert_eq!(snap2.data, b"secondl"); // "second" overwrote "initial"[0..6], "l" remains
    assert_eq!(snap2.generation, 2);

    // Now clear with matching generation — must succeed.
    table.clear_dirty(fh, snap2.generation);
    assert!(table.dirty_snapshot(fh).is_none());
}

/// Truncating a `DirectFd` handle to non-zero preserves leading bytes.
#[test]
fn truncate_direct_handle_to_nonzero_preserves_prefix() {
    let table = HandleTable::new();
    let fd = temp_file(b"hello world");
    let fh = table.open_direct(1, fd, 0);

    // Truncate to 5 — should populate from fd first, then truncate.
    table.truncate(fh, 5);

    // Write at offset 0 — should NOT re-read from fd (state is Truncated).
    table.write(fh, 0, b"HELLO").unwrap();

    let data = table.read(fh, 0, 256).unwrap();
    assert_eq!(data, b"HELLO");
}

// OpenMode::parse

#[rstest]
#[case::rdonly(0, false, false, false)]
#[case::trunc(libc::O_TRUNC, true, false, false)]
#[case::append(libc::O_APPEND, false, true, false)]
#[case::trunc_and_append(libc::O_TRUNC | libc::O_APPEND, true, true, false)]
#[case::wronly(libc::O_WRONLY, false, false, true)]
#[case::rdwr(libc::O_RDWR, false, false, true)]
#[case::shell_redirect(libc::O_WRONLY | libc::O_TRUNC, true, false, true)]
fn open_mode_parse(#[case] flags: i32, #[case] truncate: bool, #[case] append: bool, #[case] write_intent: bool) {
    let mode = OpenMode::parse(flags);
    assert_eq!(mode.truncate, truncate);
    assert_eq!(mode.append, append);
    assert_eq!(mode.write_intent, write_intent);
}

// Release / entry state

/// Release returns the entry, and `is_dirty` reflects the generation counter.
#[test]
fn release_returns_dirty_entry() {
    let table = HandleTable::new();
    let fh = table.open(1, Arc::from(b"data".as_slice()), 0);

    // Not dirty initially.
    let entry_clean = table.release(fh);
    assert!(entry_clean.is_some());
    assert!(!entry_clean.unwrap().is_dirty());

    // Open again, write, release — should be dirty.
    let fh2 = table.open(1, Arc::from(b"data".as_slice()), 0);
    table.write(fh2, 0, b"X").unwrap();
    let entry_dirty = table.release(fh2).unwrap();
    assert!(entry_dirty.is_dirty());
}

// O_TRUNC dirty state and DirtySnapshot.truncated flag

#[rstest]
#[case::nonempty_content_is_dirty(b"existing content" as &[u8], true)]
#[case::empty_content_is_not_dirty(b"" as &[u8], false)]
fn buffered_open_trunc_dirty_state(#[case] initial: &[u8], #[case] expect_dirty: bool) {
    let table = HandleTable::new();
    let fh = table.open(1, Arc::from(initial), libc::O_WRONLY | libc::O_TRUNC);

    let entry = table.release(fh).unwrap();
    assert_eq!(entry.is_dirty(), expect_dirty);
    assert!(entry.buffer.as_bytes().is_empty(), "buffer must be empty after O_TRUNC");
}

/// `echo "new" > virtualfile` (`O_TRUNC` + write): snapshot contains written
/// data and reports truncated.
#[test]
fn buffered_open_trunc_then_write_snapshot() {
    let table = HandleTable::new();
    let fh = table.open(1, Arc::from(b"old".as_slice()), libc::O_WRONLY | libc::O_TRUNC);

    table.write(fh, 0, b"new content").unwrap();

    let snapshot = table.dirty_snapshot(fh).unwrap();
    assert_eq!(snapshot.data, b"new content");
    assert!(snapshot.truncated, "O_TRUNC handle must report truncated");
}

#[test]
fn write_after_trunc_returns_replacement() {
    let table = HandleTable::new();
    let fh = table.open(1, Arc::from(b"old".as_slice()), libc::O_WRONLY | libc::O_TRUNC);

    // First write after O_TRUNC on nonempty content → Replacement.
    let outcome = table.write(fh, 0, b"new content").unwrap();
    assert!(matches!(outcome, WriteOutcome::Replacement(11)));

    // Second write → Buffered (eager flush already consumed).
    let outcome = table.write(fh, 0, b"more").unwrap();
    assert!(matches!(outcome, WriteOutcome::Buffered(4)));
}

#[test]
fn write_after_trunc_empty_content_returns_buffered() {
    let table = HandleTable::new();
    // O_TRUNC on empty content → Done (no eager flush needed).
    let fh = table.open(1, Arc::from([]), libc::O_WRONLY | libc::O_TRUNC);

    let outcome = table.write(fh, 0, b"new").unwrap();
    assert!(matches!(outcome, WriteOutcome::Buffered(3)));
}

/// Shell redirect on a virtual file: `echo "new" > body.rs`.
///
/// The kernel sends: `open(O_TRUNC)` → setattr(size=0) → write(data) → flush.
/// The ops.rs open handler passes an empty buffer (content load skipped for
/// `O_TRUNC`). The setattr must NOT mark the handle dirty — otherwise the
/// flush would send empty content before the actual write data arrives,
/// destroying the existing content in the source file.
#[test]
fn shell_redirect_trunc_open_setattr_then_write() {
    let table = HandleTable::new();
    // Step 1: ops.rs open handler passes empty content for O_TRUNC.
    let fh = table.open(1, Arc::from([]), libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC);

    // Step 2: kernel sends setattr(size=0) for the O_TRUNC.
    table.truncate(fh, 0);

    // After setattr, handle must NOT be dirty — buffer was already empty.
    assert!(
        table.dirty_snapshot(fh).is_none(),
        "setattr(size=0) on an already-empty O_TRUNC buffer must not mark dirty"
    );

    // Step 3: echo writes content.
    table.write(fh, 0, b"fn foo() { new }").unwrap();

    // Step 4: flush — must contain the written data with truncated flag.
    let snapshot = table.dirty_snapshot(fh).expect("write must mark dirty");
    assert_eq!(snapshot.data, b"fn foo() { new }");
    assert!(snapshot.truncated, "O_TRUNC handle must report truncated");
}

/// `setattr(size=0)` on a Preloaded handle with content marks dirty.
/// Covers the path where truncation arrives via `setattr` rather than open flags.
#[test]
fn setattr_truncate_on_preloaded_handle_marks_dirty() {
    let table = HandleTable::new();
    // Open without O_TRUNC — content is in buffer.
    let fh = table.open(1, Arc::from(b"some content".as_slice()), libc::O_RDWR);

    // setattr(size=0) — standalone truncation.
    table.truncate(fh, 0);

    let entry = table.release(fh).unwrap();
    assert!(entry.is_dirty(), "setattr truncation must mark dirty");
    assert!(entry.buffer.as_bytes().is_empty());
}
