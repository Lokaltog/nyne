use nyne::plugin::PluginConfig;
use rstest::rstest;

use super::*;

#[rstest]
#[case::defaults(None, true, 5)]
#[case::disabled(Some(toml::toml! { enabled = false }.into()), false, 5)]
#[case::custom_depth(Some(toml::toml! { max_depth = 8 }.into()), true, 8)]
#[case::all_overridden(Some(toml::toml! { enabled = false max_depth = 12 }.into()), false, 12)]
fn from_section(
    #[case] section: Option<toml::Value>,
    #[case] expected_enabled: bool,
    #[case] expected_max_depth: usize,
) {
    let config = Config::from_section(section.as_ref());
    assert_eq!(config.enabled, expected_enabled);
    assert_eq!(config.max_depth, expected_max_depth);
}

#[test]
fn vfs_defaults() {
    let config = Config::from_section(None);
    assert_eq!(config.vfs.dir.symbols, "symbols");
    assert_eq!(config.vfs.dir.by_kind, "by-kind");
    assert_eq!(config.vfs.dir.at_line, "at-line");
    assert_eq!(config.vfs.dir.code, "code");
    assert_eq!(config.vfs.dir.edit, "edit");
    assert_eq!(config.vfs.dir.staged, "staged");
    assert_eq!(config.vfs.file.overview, "OVERVIEW.md");
    assert_eq!(config.vfs.file.body, "body");
    assert_eq!(config.vfs.file.signature, "signature");
    assert_eq!(config.vfs.file.docstring, "docstring");
    assert_eq!(config.vfs.file.decorators, "decorators");
    assert_eq!(config.vfs.file.imports, "imports");
    assert_eq!(config.vfs.file.staged_diff, "staged.diff");
}

#[rstest]
#[case::dir_override(
    toml::toml! { [vfs.dir] symbols = "sym" }.into(),
    |c: Config| assert_eq!(c.vfs.dir.symbols, "sym"),
)]
#[case::file_override(
    toml::toml! { [vfs.file] body = "src" }.into(),
    |c: Config| assert_eq!(c.vfs.file.body, "src"),
)]
#[case::multiple_overrides(
    toml::toml! {
        [vfs.dir]
        symbols = "sym"
        by_kind = "kind"
        [vfs.file]
        overview = "INDEX.md"
    }.into(),
    |c: Config| {
        assert_eq!(c.vfs.dir.symbols, "sym");
        assert_eq!(c.vfs.dir.by_kind, "kind");
        assert_eq!(c.vfs.file.overview, "INDEX.md");
        // Unspecified fields keep defaults.
        assert_eq!(c.vfs.dir.at_line, "at-line");
        assert_eq!(c.vfs.file.body, "body");
    },
)]
fn vfs_overrides(#[case] section: toml::Value, #[case] check: fn(Config)) {
    check(Config::from_section(Some(&section)));
}
