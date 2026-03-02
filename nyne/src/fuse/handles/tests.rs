use std::io::Write as _;

use super::*;

/// Create a temp file with the given content and return a read-only `File` handle.
fn temp_file(content: &[u8]) -> File {
    let mut f = tempfile::tempfile().unwrap();
    f.write_all(content).unwrap();
    f
}

#[test]
fn write_to_buffered_handle_at_offset_zero() {
    let table = HandleTable::new();
    let fh = table.open(1, b"hello".to_vec(), 0);

    let written = table.write(fh, 0, b"HELLO").unwrap();
    assert_eq!(written, 5);

    let data = table.read(fh, 0, 256);
    assert_eq!(data, b"HELLO");
}

#[test]
fn append_to_buffered_handle_preserves_original() {
    let table = HandleTable::new();
    let fh = table.open(1, b"hello".to_vec(), 0);

    let written = table.write(fh, 5, b" world").unwrap();
    assert_eq!(written, 6);

    let data = table.read(fh, 0, 256);
    assert_eq!(data, b"hello world");
}

#[test]
fn write_to_direct_handle_populates_buffer_from_fd() {
    let table = HandleTable::new();
    let original = b"original content";
    let fd = temp_file(original);
    let fh = table.open_direct(1, fd, 0);

    // Before any write, reads come from the fd via pread.
    let data = table.read(fh, 0, 256);
    assert_eq!(data, original);

    // Append via write — should lazy-populate buffer from fd first.
    let written = table.write(fh, original.len() as u64, b" appended").unwrap();
    assert_eq!(written, 9);

    // Read full buffer — original content must be preserved, not zero-filled.
    let data = table.read(fh, 0, 256);
    assert_eq!(data, b"original content appended");
}

#[test]
fn overwrite_in_direct_handle_preserves_surrounding_content() {
    let table = HandleTable::new();
    let original = b"hello world";
    let fd = temp_file(original);
    let fh = table.open_direct(1, fd, 0);

    // Overwrite "world" with "WORLD".
    table.write(fh, 6, b"WORLD").unwrap();

    let data = table.read(fh, 0, 256);
    assert_eq!(data, b"hello WORLD");
}

#[test]
fn direct_handle_write_at_zero_on_empty_file() {
    let table = HandleTable::new();
    let fd = temp_file(b"");
    let fh = table.open_direct(1, fd, 0);

    table.write(fh, 0, b"new content").unwrap();

    let data = table.read(fh, 0, 256);
    assert_eq!(data, b"new content");
}

#[test]
fn direct_handle_marks_dirty_after_write() {
    let table = HandleTable::new();
    let fd = temp_file(b"data");
    let fh = table.open_direct(1, fd, 0);

    assert!(table.dirty_snapshot(fh).is_none());

    table.write(fh, 4, b"!").unwrap();

    let (snapshot, mode, gen_id) = table.dirty_snapshot(fh).unwrap();
    assert_eq!(snapshot, b"data!");
    assert!(matches!(mode, WriteMode::Normal));
    assert_eq!(gen_id, 1);
}

#[test]
fn append_clamps_offset_to_buffer_len() {
    let table = HandleTable::new();
    let content = b"line1\nline2\n";
    // Open with O_APPEND — kernel might send a stale offset > buffer.len().
    let fh = table.open(1, content.to_vec(), libc::O_APPEND);

    // Simulate kernel sending offset = 100 (stale i_size), but buffer is 12 bytes.
    // With O_APPEND, write() should clamp to buffer.len() = 12.
    table.write(fh, 100, b"line3\n").unwrap();

    let data = table.read(fh, 0, 256);
    assert_eq!(data, b"line1\nline2\nline3\n");
}

#[test]
fn append_direct_handle_with_stale_offset() {
    let table = HandleTable::new();
    let original = b"hello";
    let fd = temp_file(original);
    let fh = table.open_direct(1, fd, libc::O_APPEND);

    // Kernel sends stale offset (e.g. 1000) but file is only 5 bytes.
    table.write(fh, 1000, b" world").unwrap();

    let data = table.read(fh, 0, 256);
    assert_eq!(data, b"hello world");
}

#[test]
fn non_append_write_at_offset_beyond_buffer_zero_fills() {
    let table = HandleTable::new();
    let fh = table.open(1, b"abc".to_vec(), 0);

    // Without O_APPEND, writing beyond buffer should zero-fill the gap.
    table.write(fh, 5, b"xy").unwrap();

    let data = table.read(fh, 0, 256);
    assert_eq!(data, b"abc\0\0xy");
}

/// Regression: `open_direct` with `O_TRUNC` followed by truncate + write
/// must NOT repopulate the buffer from the backing fd. The old content
/// should be gone — only the new write data should remain.
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
    let data = table.read(fh, 0, 256);
    assert_eq!(data, replacement, "truncated direct handle leaked old file content");
}

/// Dirty generation prevents a concurrent write from being silently lost.
///
/// Sequence: dirty_snapshot → write → clear_dirty(old_gen)
/// The clear_dirty must NOT erase the new write's dirty state.
#[test]
fn clear_dirty_respects_generation() {
    let table = HandleTable::new();
    let fh = table.open(1, b"initial".to_vec(), 0);

    table.write(fh, 0, b"first").unwrap();

    // Snapshot captures gen_id=1.
    let (_, _, gen_id) = table.dirty_snapshot(fh).unwrap();
    assert_eq!(gen_id, 1);

    // Concurrent write bumps gen_id to 2.
    table.write(fh, 0, b"second").unwrap();

    // clear_dirty with stale gen_id=1 must NOT clear — gen_id is now 2.
    table.clear_dirty(fh, gen_id);

    // Handle must still be dirty.
    let (snapshot, _, gen2) = table.dirty_snapshot(fh).unwrap();
    assert_eq!(snapshot, b"secondl"); // "second" overwrote "initial"[0..6], "l" remains
    assert_eq!(gen2, 2);

    // Now clear with matching gen_id — must succeed.
    table.clear_dirty(fh, gen2);
    assert!(table.dirty_snapshot(fh).is_none());
}

/// Truncating a DirectFd handle to non-zero preserves leading bytes.
#[test]
fn truncate_direct_handle_to_nonzero_preserves_prefix() {
    let table = HandleTable::new();
    let fd = temp_file(b"hello world");
    let fh = table.open_direct(1, fd, 0);

    // Truncate to 5 — should populate from fd first, then truncate.
    table.truncate(fh, 5);

    // Write at offset 0 — should NOT re-read from fd (state is Truncated).
    table.write(fh, 0, b"HELLO").unwrap();

    let data = table.read(fh, 0, 256);
    assert_eq!(data, b"HELLO");
}

/// WriteMode is Truncate when the handle went through a truncation.
#[test]
fn write_mode_is_truncate_after_truncation() {
    let table = HandleTable::new();
    let fd = temp_file(b"original");
    let fh = table.open_direct(1, fd, libc::O_TRUNC);

    table.truncate(fh, 0);
    table.write(fh, 0, b"new").unwrap();

    let (_, mode, _) = table.dirty_snapshot(fh).unwrap();
    assert!(matches!(mode, WriteMode::Truncate));
}

/// WriteMode is Normal for a regular direct handle write.
#[test]
fn write_mode_is_normal_for_direct_write() {
    let table = HandleTable::new();
    let fd = temp_file(b"data");
    let fh = table.open_direct(1, fd, 0);

    table.write(fh, 4, b"!").unwrap();

    let (_, mode, _) = table.dirty_snapshot(fh).unwrap();
    assert!(matches!(mode, WriteMode::Normal));
}

/// OpenMode::parse is the single source of truth for flag interpretation.
#[test]
fn open_mode_parse() {
    // O_RDONLY (default 0) — no write intent.
    let none = OpenMode::parse(0);
    assert!(!none.truncate);
    assert!(!none.append);
    assert!(!none.write_intent);

    let trunc = OpenMode::parse(libc::O_TRUNC);
    assert!(trunc.truncate);
    assert!(!trunc.append);

    let append = OpenMode::parse(libc::O_APPEND);
    assert!(!append.truncate);
    assert!(append.append);

    let both = OpenMode::parse(libc::O_TRUNC | libc::O_APPEND);
    assert!(both.truncate);
    assert!(both.append);

    // Write-intent detection: O_WRONLY and O_RDWR.
    let wronly = OpenMode::parse(libc::O_WRONLY);
    assert!(wronly.write_intent);

    let rdwr = OpenMode::parse(libc::O_RDWR);
    assert!(rdwr.write_intent);

    let rdonly = OpenMode::parse(libc::O_RDONLY);
    assert!(!rdonly.write_intent);

    // Combined: O_WRONLY | O_TRUNC (typical shell redirect `> file`).
    let redirect = OpenMode::parse(libc::O_WRONLY | libc::O_TRUNC);
    assert!(redirect.write_intent);
    assert!(redirect.truncate);
}

/// Release returns the entry, and is_dirty reflects the generation counter.
#[test]
fn release_returns_dirty_entry() {
    let table = HandleTable::new();
    let fh = table.open(1, b"data".to_vec(), 0);

    // Not dirty initially.
    let entry_clean = table.release(fh);
    assert!(entry_clean.is_some());
    assert!(!entry_clean.unwrap().is_dirty());

    // Open again, write, release — should be dirty.
    let fh2 = table.open(1, b"data".to_vec(), 0);
    table.write(fh2, 0, b"X").unwrap();
    let entry_dirty = table.release(fh2).unwrap();
    assert!(entry_dirty.is_dirty());
}

/// Standalone truncation via `O_TRUNC` on a buffered handle with content
/// marks the handle dirty so the empty buffer is flushed on release.
/// Regression: `: > virtualfile` previously left the file unchanged.
#[test]
fn buffered_open_trunc_marks_dirty_when_content_nonempty() {
    let table = HandleTable::new();
    let fh = table.open(1, b"existing content".to_vec(), libc::O_WRONLY | libc::O_TRUNC);

    // No write — just release.
    let entry = table.release(fh).unwrap();
    assert!(entry.is_dirty(), "O_TRUNC on non-empty content must be dirty");
    assert!(entry.buffer.is_empty(), "buffer must be empty after O_TRUNC");
    assert!(
        matches!(entry.write_mode(), WriteMode::Truncate),
        "write_mode must be Truncate for O_TRUNC open",
    );
}

/// `O_TRUNC` on an already-empty buffered handle is a no-op — not dirty.
#[test]
fn buffered_open_trunc_on_empty_content_is_not_dirty() {
    let table = HandleTable::new();
    let fh = table.open(1, Vec::new(), libc::O_WRONLY | libc::O_TRUNC);

    let entry = table.release(fh).unwrap();
    assert!(!entry.is_dirty(), "O_TRUNC on empty content should not be dirty");
}

/// `echo "new" > virtualfile` (O_TRUNC + write) uses WriteMode::Truncate.
/// Regression: Preloaded handles previously returned WriteMode::Normal.
#[test]
fn buffered_open_trunc_then_write_uses_truncate_mode() {
    let table = HandleTable::new();
    let fh = table.open(1, b"old".to_vec(), libc::O_WRONLY | libc::O_TRUNC);

    table.write(fh, 0, b"new content").unwrap();

    let (data, mode, _) = table.dirty_snapshot(fh).unwrap();
    assert_eq!(data, b"new content");
    assert!(matches!(mode, WriteMode::Truncate));
}

/// Shell redirect on a virtual file: `echo "new" > body.rs`.
///
/// The kernel sends: open(O_TRUNC) → setattr(size=0) → write(data) → flush.
/// The ops.rs open handler passes an empty buffer (content load skipped for
/// O_TRUNC). The setattr must NOT mark the handle dirty — otherwise the
/// flush would send empty content before the actual write data arrives,
/// destroying the existing content in the source file.
#[test]
fn shell_redirect_trunc_open_setattr_then_write() {
    let table = HandleTable::new();
    // Step 1: ops.rs open handler passes empty content for O_TRUNC.
    let fh = table.open(1, Vec::new(), libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC);

    // Step 2: kernel sends setattr(size=0) for the O_TRUNC.
    table.truncate(fh, 0);

    // After setattr, handle must NOT be dirty — buffer was already empty.
    assert!(
        table.dirty_snapshot(fh).is_none(),
        "setattr(size=0) on an already-empty O_TRUNC buffer must not mark dirty"
    );

    // Step 3: echo writes content.
    table.write(fh, 0, b"fn foo() { new }").unwrap();

    // Step 4: flush — must contain the written data with Truncate mode.
    let (data, mode, _) = table.dirty_snapshot(fh).expect("write must mark dirty");
    assert_eq!(data, b"fn foo() { new }");
    assert!(matches!(mode, WriteMode::Truncate));
}

/// `setattr(size=0)` on a Preloaded handle with content marks dirty.
/// Covers the path where truncation arrives via `setattr` rather than open flags.
#[test]
fn setattr_truncate_on_preloaded_handle_marks_dirty() {
    let table = HandleTable::new();
    // Open without O_TRUNC — content is in buffer.
    let fh = table.open(1, b"some content".to_vec(), libc::O_RDWR);

    // setattr(size=0) — standalone truncation.
    table.truncate(fh, 0);

    let entry = table.release(fh).unwrap();
    assert!(entry.is_dirty(), "setattr truncation must mark dirty");
    assert!(entry.buffer.is_empty());
    assert!(matches!(entry.write_mode(), WriteMode::Truncate));
}
