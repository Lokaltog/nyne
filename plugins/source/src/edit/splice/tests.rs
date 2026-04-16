use std::path::PathBuf;

use nyne::router::fs::os::OsFilesystem;

use super::*;
use crate::test_support::splice_validate_write;

/// Creates a temp file with the given content and returns the test context.
fn setup(content: &str) -> (tempfile::TempDir, OsFilesystem, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("test.rs"), content).unwrap();
    let fs = OsFilesystem::new(dir.path());
    (dir, fs, PathBuf::from("test.rs"))
}

/// Validation function that always succeeds.
fn always_ok(_: &str) -> Result<(), String> { Ok(()) }

/// Validation function that rejects source containing "`SYNTAX_ERROR`".
fn reject_if_contains_error(source: &str) -> Result<(), String> {
    if source.contains("SYNTAX_ERROR") {
        Err("source contains SYNTAX_ERROR".to_owned())
    } else {
        Ok(())
    }
}

/// Tests that splicing valid content into a valid file succeeds.
#[test]
fn splice_valid_to_valid_succeeds() {
    let (_dir, fs, path) = setup("fn hello() {}");
    let result = splice_validate_write(&fs, &path, 3..8, "world", always_ok);
    assert!(result.is_ok());
    let written = fs.read_file(&path).unwrap();
    assert_eq!(std::str::from_utf8(&written).unwrap(), "fn world() {}\n");
}

/// Tests that splicing invalid content into a valid file is rejected.
#[test]
fn splice_valid_to_invalid_is_rejected() {
    let (_dir, fs, path) = setup("fn hello() {}");
    let result = splice_validate_write(&fs, &path, 3..8, "SYNTAX_ERROR", reject_if_contains_error);
    assert!(result.is_err());
    // Original file should be unchanged.
    let written = fs.read_file(&path).unwrap();
    assert_eq!(std::str::from_utf8(&written).unwrap(), "fn hello() {}");
}

/// Tests that splicing into an already-invalid file is allowed even when
/// the spliced result is also invalid.
///
/// This exercises the pre-splice validation fallback in
/// [`splice_rope_validate_write`]: when post-splice validation fails, the
/// fallback re-validates the pre-splice rope (cheaply cloned, O(1)) and
/// suppresses the error if the source was already invalid.
///
/// The splice keeps the `SYNTAX_ERROR` marker in the output so that
/// post-splice validation fails, forcing the fallback branch to fire
/// rather than trivially passing on a clean spliced result.
#[test]
fn splice_already_invalid_file_allows_write() {
    let (_dir, fs, path) = setup("fn SYNTAX_ERROR_old() {}");
    // Replace "old" with "new" at bytes 16..19 — both pre- and post-splice
    // content contain `SYNTAX_ERROR`, so `validate(&spliced)` errors and
    // the pre-splice rope clone is what rescues the write.
    assert!(
        splice_validate_write(&fs, &path, 16..19, "new", reject_if_contains_error).is_ok(),
        "pre-splice-invalid fallback should allow the write",
    );
    assert_eq!(
        std::str::from_utf8(&fs.read_file(&path).unwrap()).unwrap(),
        "fn SYNTAX_ERROR_new() {}\n",
    );
}

/// Tests that an out-of-bounds splice range is rejected.
#[test]
fn splice_out_of_bounds_is_rejected() {
    let (_dir, fs, path) = setup("short");
    let result = splice_validate_write(&fs, &path, 0..100, "x", always_ok);
    assert!(result.is_err());
}

/// Splicing content that removes the trailing newline must re-add it.
/// POSIX text file convention; tree-sitter-markdown rejects files without it.
#[test]
fn splice_ensures_trailing_newline() {
    let (_dir, fs, path) = setup("hello\n");
    let result = splice_validate_write(&fs, &path, 0..6, "world", always_ok);
    assert!(result.is_ok());
    let written = fs.read_file(&path).unwrap();
    assert_eq!(std::str::from_utf8(&written).unwrap(), "world\n");
}

/// Splicing into content that already has a trailing newline does not double it.
#[test]
fn splice_does_not_double_trailing_newline() {
    let (_dir, fs, path) = setup("hello\n");
    let result = splice_validate_write(&fs, &path, 0..6, "world\n", always_ok);
    assert!(result.is_ok());
    let written = fs.read_file(&path).unwrap();
    assert_eq!(std::str::from_utf8(&written).unwrap(), "world\n");
}

/// A splice whose new content is byte-identical to the existing slice is a
/// no-op — validation is skipped and the file is not re-written.
///
/// This prevents cascading cache invalidations from round-trips like
/// `cat body.rs > body.rs` where downstream providers would otherwise
/// evict the kernel page cache for an unchanged file.
#[test]
fn splice_noop_skips_validate_and_write() {
    let (_dir, fs, path) = setup("hello world\n");
    let validate_calls = std::cell::Cell::new(0);
    let counting_validate = |_: &str| -> Result<(), String> {
        validate_calls.set(validate_calls.get() + 1);
        Ok(())
    };
    let mut rope = crop::Rope::from("hello world\n");
    // Splice "world" with "world" — byte-identical, trailing newline already
    // present. The fast path should return without invoking validate.
    let result = splice_rope_validate_write(&fs, &path, &mut rope, 6..11, "world", counting_validate);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "world".len());
    assert_eq!(validate_calls.get(), 0, "no-op splice must not invoke validate");
    // File contents unchanged.
    let written = fs.read_file(&path).unwrap();
    assert_eq!(std::str::from_utf8(&written).unwrap(), "hello world\n");
}
