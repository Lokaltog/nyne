use nyne::plugin::PluginConfig;
use rstest::rstest;

use super::*;

/// Loads a fixture TOML file from the `fixtures/` directory, parses the `[lsp]`
/// table, and deserializes it into [`Config`].
fn load_fixture(name: &str) -> Config {
    let content = nyne::load_fixture!("plugin/config", name);
    let table: toml::Table = toml::from_str(&content).unwrap_or_else(|e| panic!("failed to parse fixture {name}: {e}"));
    Config::from_section(table.get("lsp"))
}

/// Verifies that a missing plugin section returns default config.
#[test]
fn missing_plugin_section_returns_defaults() {
    assert!(Config::from_section(None).enabled);
}

/// Verifies that each fixture file deserializes without error.
#[rstest]
#[case::disabled("lsp_disabled.toml")]
#[case::empty_section("lsp_empty_section.toml")]
#[case::full("lsp_full.toml")]
#[case::custom_durations("lsp_custom_durations.toml")]
#[case::custom_server("lsp_custom_server.toml")]
#[case::custom_server_no_args("lsp_custom_server_no_args.toml")]
#[case::multiple_custom_servers("lsp_multiple_custom_servers.toml")]
#[case::server_disable("lsp_server_disable.toml")]
#[case::server_override_args("lsp_server_override_args.toml")]
#[case::server_override_command("lsp_server_override_command.toml")]
fn fixture_deserializes(#[case] fixture: &str) { let _config = load_fixture(fixture); }

/// Verifies that `enabled = false` is correctly deserialized.
#[test]
fn disabled_config() {
    assert!(!load_fixture("lsp_disabled.toml").enabled);
}

/// Verifies that custom durations are parsed correctly.
#[test]
fn custom_durations() {
    let config = load_fixture("lsp_custom_durations.toml");
    assert_eq!(config.cache_ttl, std::time::Duration::from_secs(600));
    assert_eq!(config.diagnostics_timeout, std::time::Duration::from_secs(5));
}

/// Verifies that the full config fixture parses with overridden values.
#[test]
fn full_config() {
    let config = load_fixture("lsp_full.toml");
    assert!(config.enabled);
    assert_eq!(config.cache_ttl, std::time::Duration::from_secs(600));
    assert_eq!(config.diagnostics_timeout, std::time::Duration::from_secs(3));

    // rust-analyzer should have overridden args
    let ra = &config.servers["rust-analyzer"];
    assert_eq!(
        ra.args.as_deref(),
        Some(["--log-file", "/tmp/ra.log"].map(String::from).as_slice())
    );

    // basedpyright should be disabled
    assert!(!config.servers["basedpyright"].enabled);

    // ruff is a custom server addition
    assert_eq!(config.servers["ruff"].command.as_deref(), Some("ruff"));
}

/// Verifies that disabling a built-in server works.
#[test]
fn server_disable() {
    assert!(!load_fixture("lsp_server_disable.toml").servers["basedpyright"].enabled);
}

#[test]
fn vfs_defaults() {
    let config = Config::from_section(None);
    assert_eq!(config.vfs.file.diagnostics, "DIAGNOSTICS.md");
    assert_eq!(config.vfs.dir.actions, "actions");
    assert_eq!(config.vfs.dir.rename, "rename");
    assert_eq!(config.vfs.dir.search, "search");
}

#[rstest]
#[case::file_override(
    toml::toml! { [vfs.file] diagnostics = "DIAG.md" }.into(),
    |c: Config| assert_eq!(c.vfs.file.diagnostics, "DIAG.md"),
)]
#[case::dir_override(
    toml::toml! { [vfs.dir] actions = "code-actions" }.into(),
    |c: Config| assert_eq!(c.vfs.dir.actions, "code-actions"),
)]
#[case::multiple_overrides(
    toml::toml! {
        [vfs.dir]
        rename = "refactor"
        search = "find"
    }.into(),
    |c: Config| {
        assert_eq!(c.vfs.dir.rename, "refactor");
        assert_eq!(c.vfs.dir.search, "find");
        // Unspecified fields keep defaults.
        assert_eq!(c.vfs.file.diagnostics, "DIAGNOSTICS.md");
    },
)]
fn vfs_overrides(#[case] section: toml::Value, #[case] check: fn(Config)) {
    check(Config::from_section(Some(&section)));
}
