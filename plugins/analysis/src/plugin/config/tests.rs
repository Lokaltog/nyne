use nyne::config::PluginConfig;
use rstest::rstest;

use super::*;

#[test]
fn vfs_defaults() {
    let config = Config::from_section(None);
    assert_eq!(config.vfs.file.analysis, "ANALYSIS.md");
}

#[rstest]
#[case::override_name(
    toml::toml! { [vfs.file] analysis = "HINTS.md" }.into(),
    |c: Config| assert_eq!(c.vfs.file.analysis, "HINTS.md"),
)]
#[case::empty_vfs_keeps_defaults(
    toml::toml! { [vfs.file] }.into(),
    |c: Config| assert_eq!(c.vfs.file.analysis, "ANALYSIS.md"),
)]
fn vfs_overrides(#[case] section: toml::Value, #[case] check: fn(Config)) {
    check(Config::from_section(Some(&section)));
}
