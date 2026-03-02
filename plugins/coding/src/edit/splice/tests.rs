use nyne::types::real_fs::OsFs;
use nyne::types::vfs_path::VfsPath;

use super::*;

fn setup(content: &str) -> (tempfile::TempDir, OsFs, VfsPath) {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.rs");
    std::fs::write(&file_path, content).unwrap();
    let fs = OsFs::new(dir.path().to_path_buf());
    let vfs_path = VfsPath::new("test.rs").unwrap();
    (dir, fs, vfs_path)
}

fn always_ok(_: &str) -> Result<(), String> { Ok(()) }

fn reject_if_contains_error(source: &str) -> Result<(), String> {
    if source.contains("SYNTAX_ERROR") {
        Err("source contains SYNTAX_ERROR".to_owned())
    } else {
        Ok(())
    }
}

#[test]
fn splice_valid_to_valid_succeeds() {
    let (_dir, fs, path) = setup("fn hello() {}");
    let result = splice_validate_write(&fs, &path, 3..8, "world", always_ok);
    assert!(result.is_ok());
    let written = fs.read(&path).unwrap();
    assert_eq!(std::str::from_utf8(&written).unwrap(), "fn world() {}");
}

#[test]
fn splice_valid_to_invalid_is_rejected() {
    let (_dir, fs, path) = setup("fn hello() {}");
    let result = splice_validate_write(&fs, &path, 3..8, "SYNTAX_ERROR", reject_if_contains_error);
    assert!(result.is_err());
    // Original file should be unchanged.
    let written = fs.read(&path).unwrap();
    assert_eq!(std::str::from_utf8(&written).unwrap(), "fn hello() {}");
}

#[test]
fn splice_already_invalid_file_allows_write() {
    let (_dir, fs, path) = setup("fn SYNTAX_ERROR() {}");
    // File already contains the error marker — validation would fail on the
    // original. The splice itself doesn't introduce new errors, but the point
    // is that validation is skipped entirely for already-invalid files.
    let result = splice_validate_write(&fs, &path, 3..15, "still_broken", reject_if_contains_error);
    assert!(result.is_ok());
    let written = fs.read(&path).unwrap();
    assert_eq!(std::str::from_utf8(&written).unwrap(), "fn still_broken() {}");
}

#[test]
fn splice_out_of_bounds_is_rejected() {
    let (_dir, fs, path) = setup("short");
    let result = splice_validate_write(&fs, &path, 0..100, "x", always_ok);
    assert!(result.is_err());
}
