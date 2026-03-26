use rstest::rstest;

use super::*;

#[rstest]
#[case::simple("file.rs@/symbols/Foo@/body.rs", Some("Foo"))]
#[case::nested("dir/file.rs@/symbols/MyImpl@/process@/body.rs", Some("MyImpl"))]
#[case::no_symbols("file.rs@/OVERVIEW.md", None)]
#[case::empty_symbol("file.rs@/symbols/@/body.rs", None)]
#[case::plain_path("src/lib.rs", None)]
fn symbol_from_vfs_path_cases(#[case] path: &str, #[case] expected: Option<&str>) {
    assert_eq!(symbol_from_vfs_path(path), expected);
}

#[rstest]
#[case::overview("file.rs@/symbols/OVERVIEW.md", true)]
#[case::nested_overview("dir/file.rs@/symbols/Foo@/OVERVIEW.md", true)]
#[case::non_overview("file.rs@/symbols/Foo@/body.rs", false)]
#[case::plain_overview("OVERVIEW.md", false)]
#[case::at_overview("file.rs@/OVERVIEW.md", false)]
fn is_symbols_overview_cases(#[case] path: &str, #[case] expected: bool) {
    assert_eq!(is_symbols_overview(path), expected);
}
