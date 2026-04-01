use nyne::config::PluginConfig;
use rstest::rstest;

use super::*;

#[test]
fn vfs_defaults() {
    let config = Config::from_section(None);
    assert_eq!(config.vfs.todo, "todo");
    assert_eq!(config.vfs.overview, "OVERVIEW.md");
}

#[rstest]
#[case::dir_override(
    toml::toml! { [vfs] todo = "tasks" }.into(),
    |c: Config| assert_eq!(c.vfs.todo, "tasks"),
)]
#[case::file_override(
    toml::toml! { [vfs] overview = "INDEX.md" }.into(),
    |c: Config| assert_eq!(c.vfs.overview, "INDEX.md"),
)]
#[case::multiple_overrides(
    toml::toml! {
        [vfs]
        todo = "tasks"
        overview = "INDEX.md"
    }.into(),
    |c: Config| {
        assert_eq!(c.vfs.todo, "tasks");
        assert_eq!(c.vfs.overview, "INDEX.md");
    },
)]
fn vfs_overrides(#[case] section: toml::Value, #[case] check: fn(Config)) {
    check(Config::from_section(Some(&section)));
}
