use std::path::PathBuf;

use nyne::router::fs::os::OsFilesystem;

use super::*;

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

/// Tests that splicing into an already-invalid file skips validation.
#[test]
fn splice_already_invalid_file_allows_write() {
    let (_dir, fs, path) = setup("fn SYNTAX_ERROR() {}");
    // File already contains the error marker — validation would fail on the
    // original. The splice itself doesn't introduce new errors, but the point
    // is that validation is skipped entirely for already-invalid files.
    let result = splice_validate_write(&fs, &path, 3..15, "still_broken", reject_if_contains_error);
    assert!(result.is_ok());
    let written = fs.read_file(&path).unwrap();
    assert_eq!(std::str::from_utf8(&written).unwrap(), "fn still_broken() {}\n");
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
