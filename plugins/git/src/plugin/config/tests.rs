use nyne::plugin::PluginConfig;
use rstest::rstest;

use super::*;

#[test]
fn vfs_defaults() {
    let config = Config::from_section(None);
    assert_eq!(config.limits.history, 50);
    assert_eq!(config.limits.log, 200);
    assert_eq!(config.limits.notes, 50);
    assert_eq!(config.limits.contributors, 500);
    assert_eq!(config.limits.recent_commits, 10);
    assert_eq!(config.vfs.dir.git, "git");
    assert_eq!(config.vfs.dir.branches, "branches");
    assert_eq!(config.vfs.dir.tags, "tags");
    assert_eq!(config.vfs.dir.history, "history");
    assert_eq!(config.vfs.dir.diff, "diff");
    assert_eq!(config.vfs.file.blame, "BLAME.md");
    assert_eq!(config.vfs.file.log, "LOG.md");
    assert_eq!(config.vfs.file.contributors, "CONTRIBUTORS.md");
    assert_eq!(config.vfs.file.notes, "NOTES.md");
    assert_eq!(config.vfs.file.status, "STATUS.md");
    assert_eq!(config.vfs.file.head_diff, "HEAD.diff");
}

#[rstest]
#[case::dir_override(
    toml::toml! { [vfs.dir] git = "vcs" }.into(),
    |c: Config| assert_eq!(c.vfs.dir.git, "vcs"),
)]
#[case::file_override(
    toml::toml! { [vfs.file] blame = "ANNOTATE.md" }.into(),
    |c: Config| assert_eq!(c.vfs.file.blame, "ANNOTATE.md"),
)]
#[case::limits_history_override(
    toml::toml! { [limits] history = 100 }.into(),
    |c: Config| {
        assert_eq!(c.limits.history, 100);
        // Unspecified limits keep defaults.
        assert_eq!(c.limits.log, 200);
        assert_eq!(c.limits.notes, 50);
    },
)]
#[case::limits_multiple(
    toml::toml! {
        [limits]
        log = 1000
        contributors = 50
        recent_commits = 25
    }.into(),
    |c: Config| {
        assert_eq!(c.limits.log, 1000);
        assert_eq!(c.limits.contributors, 50);
        assert_eq!(c.limits.recent_commits, 25);
        assert_eq!(c.limits.history, 50);
    },
)]
#[case::multiple_overrides(
    toml::toml! {
        [vfs.dir]
        git = "vcs"
        branches = "br"
        [vfs.file]
        log = "HISTORY.md"
    }.into(),
    |c: Config| {
        assert_eq!(c.vfs.dir.git, "vcs");
        assert_eq!(c.vfs.dir.branches, "br");
        assert_eq!(c.vfs.file.log, "HISTORY.md");
        // Unspecified fields keep defaults.
        assert_eq!(c.vfs.dir.tags, "tags");
        assert_eq!(c.vfs.file.blame, "BLAME.md");
        assert_eq!(c.limits.history, 50);
    },
)]
fn vfs_overrides(#[case] section: toml::Value, #[case] check: fn(Config)) {
    check(Config::from_section(Some(&section)));
}
