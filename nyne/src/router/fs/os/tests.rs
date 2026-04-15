use std::fs;
use std::os::unix::fs::symlink;

use rstest::{fixture, rstest};
use tempfile::TempDir;

use super::*;

/// Build an `OsFilesystem` rooted at a fresh tempdir populated with one entry
/// of each kind — regular file, directory, symlink to file, symlink to
/// directory, and dangling symlink.
#[fixture]
fn populated() -> (TempDir, OsFilesystem) {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let root = tmp.path().to_path_buf();
    fs::write(root.join("file.txt"), b"hello").unwrap();
    fs::create_dir(root.join("subdir")).unwrap();
    symlink("file.txt", root.join("link_to_file")).unwrap();
    symlink("subdir", root.join("link_to_dir")).unwrap();
    symlink("missing-target", root.join("dangling_link")).unwrap();
    (tmp, OsFilesystem::new(root))
}

/// `read_dir` must tag every entry with the correct `NodeKind`, including
/// symlinks — which must never be bucketed into `NodeKind::File` regardless
/// of what their target points at. Prior bug: `read_dir` only checked
/// `is_dir()` and fell through to `NodeKind::File` for everything else.
#[rstest]
#[case::regular_file("file.txt", NodeKind::File)]
#[case::directory("subdir", NodeKind::Directory)]
#[case::symlink_to_file("link_to_file", NodeKind::Symlink)]
#[case::symlink_to_directory("link_to_dir", NodeKind::Symlink)]
#[case::dangling_symlink("dangling_link", NodeKind::Symlink)]
fn read_dir_kind(populated: (TempDir, OsFilesystem), #[case] name: &str, #[case] expected: NodeKind) {
    let (_tmp, fs) = populated;
    assert_eq!(
        fs.read_dir(Path::new(""))
            .unwrap()
            .into_iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("entry {name} not found"))
            .kind,
        expected,
    );
}

/// `stat` must not follow symlinks when determining kind, and must not
/// lose dangling symlinks. Prior bugs: `fs::metadata` was used (follows
/// links, so mis-reports the target's kind), and dangling links returned
/// `None` because `metadata` errored with `NotFound`.
#[rstest]
#[case::regular_file("file.txt", NodeKind::File)]
#[case::directory("subdir", NodeKind::Directory)]
#[case::symlink_to_file("link_to_file", NodeKind::Symlink)]
#[case::symlink_to_directory("link_to_dir", NodeKind::Symlink)]
#[case::dangling_symlink("dangling_link", NodeKind::Symlink)]
fn stat_kind(populated: (TempDir, OsFilesystem), #[case] name: &str, #[case] expected: NodeKind) {
    let (_tmp, fs) = populated;
    assert_eq!(
        fs.stat(Path::new(""), name)
            .unwrap()
            .unwrap_or_else(|| panic!("stat({name}) returned None"))
            .kind,
        expected,
    );
}

/// `metadata` must not follow symlinks — size should be the symlink text
/// length, not the target's size, and kind should be `Symlink`.
#[rstest]
#[case::symlink_to_file("link_to_file", "file.txt")]
#[case::symlink_to_directory("link_to_dir", "subdir")]
#[case::dangling_symlink("dangling_link", "missing-target")]
fn metadata_kind_and_size(populated: (TempDir, OsFilesystem), #[case] name: &str, #[case] target: &str) {
    let (_tmp, fs) = populated;
    let meta = fs.metadata(Path::new(name)).unwrap();
    assert_eq!(meta.file_type, NodeKind::Symlink);
    assert_eq!(meta.size, target.len() as u64);
}

/// `symlink_target` returns the raw target as stored in the link — no
/// resolution, no existence check.
#[rstest]
#[case::to_file("link_to_file", "file.txt")]
#[case::to_directory("link_to_dir", "subdir")]
#[case::dangling("dangling_link", "missing-target")]
fn symlink_target_roundtrip(populated: (TempDir, OsFilesystem), #[case] name: &str, #[case] expected: &str) {
    let (_tmp, fs) = populated;
    assert_eq!(fs.symlink_target(Path::new(name)).unwrap(), Path::new(expected));
}
