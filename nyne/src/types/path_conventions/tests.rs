use rstest::rstest;

use super::*;

#[rstest]
#[case::vfs_path("src/lib.rs@/symbols/Foo@/body.rs", true)]
#[case::plain_path("src/lib.rs", false)]
#[case::at_no_slash("src/lib.rs@", false)]
#[case::root_companion("@/OVERVIEW.md", true)]
fn is_vfs_path_cases(#[case] path: &str, #[case] expected: bool) {
    assert_eq!(is_vfs_path(path), expected);
}

#[rstest]
#[case::vfs_path("src/lib.rs@/symbols/Foo@/body.rs", "src/lib.rs")]
#[case::plain_path("src/lib.rs", "src/lib.rs")]
#[case::nested_vfs("dir/file.rs@/symbols/at-line/42", "dir/file.rs")]
fn source_file_of_cases(#[case] path: &str, #[case] expected: &str) {
    assert_eq!(source_file_of(path), expected);
}

#[test]
fn vfs_separator_matches_companion_suffix() {
    assert_eq!(VFS_SEPARATOR, format!("{COMPANION_SUFFIX}/"));
}
