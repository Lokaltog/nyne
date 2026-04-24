use rstest::rstest;

use super::*;

#[rstest]
fn default_uses_canonical_names() {
    // `at_line` formatting is covered by `at_line_formats_numbers` — this test pins the
    // root `symbols` dir name, which has no parametrized coverage elsewhere.
    assert_eq!(SourcePaths::default().symbols_dir(), "symbols");
}

#[rstest]
fn custom_config_propagates() {
    let dirs = VfsDirs {
        symbols: "sym".into(),
        at_line: "line".into(),
        ..VfsDirs::default()
    };
    let paths = SourcePaths::from_vfs(&dirs);
    assert_eq!(paths.symbols_dir(), "sym");
    assert_eq!(paths.at_line(1), "sym/line/1");
}

#[rstest]
#[case::small(42, "symbols/at-line/42")]
#[case::large(99999, "symbols/at-line/99999")]
fn at_line_formats_numbers(#[case] line: usize, #[case] expected: &str) {
    let paths = SourcePaths::default();
    assert_eq!(paths.at_line(line), expected);
}
