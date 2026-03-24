use rstest::rstest;

use super::*;
use crate::test_support::vfs;

/// Tests that extension extracts the part after the last dot.
#[rstest]
#[case::rust_file("src/main.rs", Some("rs"))]
#[case::no_extension("Makefile", None)]
#[case::multiple_dots("archive.tar.gz", Some("gz"))]
fn extension(#[case] path: &str, #[case] expected: Option<&str>) {
    assert_eq!(vfs(path).extension(), expected);
}

/// Verifies that the root path has no extension.
#[test]
fn root_path_has_no_extension() {
    assert_eq!(VfsPath::root().extension(), None);
}

/// Tests that compound_extension extracts the two rightmost extension segments.
#[rstest]
#[case::two_dots("template.md.j2", Some(("md", "j2")))]
#[case::toml_j2("config.toml.j2", Some(("toml", "j2")))]
#[case::single_ext("main.rs", None)]
#[case::no_ext("Makefile", None)]
#[case::three_dots("archive.tar.gz", Some(("tar", "gz")))]
#[case::empty_inner("foo..j2", None)]
fn compound_extension(#[case] path: &str, #[case] expected: Option<(&str, &str)>) {
    assert_eq!(vfs(path).compound_extension(), expected);
}

/// Verifies that the root path has no compound extension.
#[test]
fn root_path_has_no_compound_extension() {
    assert_eq!(VfsPath::root().compound_extension(), None);
}

/// Tests that relative_to computes correct relative paths between two VfsPaths.
#[rstest]
#[case::same_parent("symbols/Foo@/body.rs", "symbols", "Foo@/body.rs")]
#[case::one_level_up("symbols/Foo@/body.rs", "symbols/at-line", "../Foo@/body.rs")]
#[case::two_levels_up("symbols/Foo@/body.rs", "symbols/by-kind/function", "../../Foo@/body.rs")]
#[case::disjoint_trees("symbols/at-line/42", "todo/TODO", "../../symbols/at-line/42")]
#[case::cross_file(
    "src/lib.rs@/symbols/Bar@",
    "src/main.rs@/symbols/Foo@/callers",
    "../../../../lib.rs@/symbols/Bar@"
)]
#[case::identical_paths("symbols/Foo@", "symbols/Foo@", "")]
#[case::target_is_root("", "symbols/at-line", "../..")]
#[case::base_is_root("symbols/at-line/42", "", "symbols/at-line/42")]
fn relative_to(#[case] target: &str, #[case] base: &str, #[case] expected: &str) {
    let target = if target.is_empty() {
        VfsPath::root()
    } else {
        vfs(target)
    };
    let base = if base.is_empty() { VfsPath::root() } else { vfs(base) };
    assert_eq!(target.relative_to(&base), std::path::PathBuf::from(expected));
}
