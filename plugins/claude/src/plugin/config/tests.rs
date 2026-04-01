use std::collections::HashMap;

use nyne::config::PluginConfig;
use rstest::rstest;

use super::*;

/// Parses a TOML string into a `Config` for testing.
fn parse_config(toml_str: &str) -> Config {
    let value: toml::Value = toml::from_str(toml_str).unwrap();
    Config::from_section(Some(&value))
}

/// Loads a `Config` from a named test fixture file.
fn load_fixture(name: &str) -> Config {
    let path = format!("{}/src/plugin/config/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let content = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"));
    parse_config(&content)
}

#[rstest]
#[case::rust("Rust", 60)]
#[case::python("Python", 60)]
#[case::typescript("TypeScript", 60)]
#[case::markdown("Markdown", -1)]
#[case::toml_lang("TOML", -1)]
fn builtin_deny_threshold(#[case] lang: &str, #[case] expected: i64) {
    assert_eq!(
        PreToolHookConfig::default().resolve(lang).deny_lines_threshold(),
        expected
    );
}

#[rstest]
#[case::rust("Rust")]
#[case::markdown("Markdown")]
fn builtin_narrow_read_limit(#[case] lang: &str) {
    assert_eq!(PreToolHookConfig::default().resolve(lang).narrow_read_limit(), 80);
}

#[rstest]
#[case::rust("Rust")]
#[case::markdown("Markdown")]
fn builtin_no_symbol_table(#[case] lang: &str) {
    assert!(!PreToolHookConfig::default().resolve(lang).include_symbol_table.unwrap());
}

#[rstest]
#[case::upper("MARKDOWN", -1)]
#[case::lower("markdown", -1)]
#[case::mixed("Markdown", -1)]
fn builtin_defaults_case_insensitive(#[case] lang: &str, #[case] expected: i64) {
    assert_eq!(
        PreToolHookConfig::default().resolve(lang).deny_lines_threshold(),
        expected
    );
}

#[test]
fn merge_null_preserves_base() {
    let merged = PreToolPolicy {
        deny_lines_threshold: Some(42),
        narrow_read_limit: Some(100),
        include_symbol_table: Some(true),
    }
    .merge(&PreToolPolicy::default());

    assert_eq!(merged.deny_lines_threshold, Some(42));
    assert_eq!(merged.narrow_read_limit, Some(100));
    assert_eq!(merged.include_symbol_table, Some(true));
}

#[test]
fn merge_overwrites_non_null() {
    let merged = PreToolPolicy {
        deny_lines_threshold: Some(60),
        narrow_read_limit: Some(80),
        include_symbol_table: Some(false),
    }
    .merge(&PreToolPolicy {
        deny_lines_threshold: Some(-1),
        include_symbol_table: Some(true),
        ..Default::default()
    });

    assert_eq!(merged.deny_lines_threshold, Some(-1));
    assert_eq!(merged.narrow_read_limit, Some(80));
    assert_eq!(merged.include_symbol_table, Some(true));
}

#[test]
fn user_default_overrides_builtin() {
    let config = PreToolHookConfig {
        default: PreToolPolicy {
            deny_lines_threshold: Some(100),
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(config.resolve("Rust").deny_lines_threshold(), 100);
    assert_eq!(config.resolve("Markdown").deny_lines_threshold(), 100);
}

#[test]
fn filetype_overrides_user_default() {
    let config = PreToolHookConfig {
        default: PreToolPolicy {
            deny_lines_threshold: Some(100),
            ..Default::default()
        },
        filetype: HashMap::from([("markdown".into(), PreToolPolicy {
            deny_lines_threshold: Some(-1),
            ..Default::default()
        })]),
    };
    assert_eq!(config.resolve("Markdown").deny_lines_threshold(), -1);
    assert_eq!(config.resolve("Rust").deny_lines_threshold(), 100);
}

#[test]
fn partial_filetype_inherits_unset_fields() {
    let policy = PreToolHookConfig {
        filetype: HashMap::from([("rust".into(), PreToolPolicy {
            include_symbol_table: Some(true),
            ..Default::default()
        })]),
        ..Default::default()
    }
    .resolve("Rust");

    assert!(policy.include_symbol_table.unwrap());
    assert_eq!(policy.deny_lines_threshold(), 60);
    assert_eq!(policy.narrow_read_limit(), 80);
}

#[test]
fn config_from_pre_tool_fixture() {
    let config = load_fixture("pre_tool_overrides.toml");

    let md = config.hook_config.pre_tool.resolve("Markdown");
    assert_eq!(md.deny_lines_threshold(), -1);
    assert!(md.include_symbol_table.unwrap());

    let rs = config.hook_config.pre_tool.resolve("Rust");
    assert_eq!(rs.deny_lines_threshold(), 100);
    assert!(!rs.include_symbol_table.unwrap());
}

#[test]
fn stop_defaults() {
    let stop = &Config::default().hook_config.stop;
    assert_eq!(stop.min_files, 2);
    assert!(stop.ignore_extensions.contains(&"toml".to_owned()));
    assert!(stop.ignore_extensions.contains(&"md".to_owned()));
    assert!(!stop.ignore_extensions.contains(&"rs".to_owned()));
}

#[test]
fn stop_config_from_fixture() {
    let stop = load_fixture("stop_overrides.toml").hook_config.stop;
    assert_eq!(stop.min_files, 3);
    assert_eq!(stop.ignore_extensions, vec!["toml", "md"]);
}

#[test]
fn claude_defaults_all_enabled() {
    let config = Config::default();
    assert!(config.enabled);
    assert!(config.hooks.session_start);
    assert!(config.hooks.pre_tool_use);
    assert!(config.hooks.post_tool_use);
    assert!(config.hooks.stop);
    assert!(config.hooks.statusline);
}

#[test]
fn claude_overrides_from_fixture() {
    let config = load_fixture("claude_overrides.toml");
    assert!(config.enabled);
    assert!(!config.hooks.statusline);
    assert!(!config.hooks.stop);
    assert!(config.hooks.session_start);
}

#[test]
fn missing_section_returns_defaults() {
    let config = Config::from_section(None);
    assert!(config.enabled);
    assert_eq!(config.hook_config.pre_tool.resolve("Rust").deny_lines_threshold(), 60);
}

#[rstest]
#[case::unknown_hook_field("[hook_config]\nbogus = true")]
#[case::unknown_pre_tool_field("[hook_config.pre_tool]\nbogus = true")]
#[case::unknown_filetype_field("[hook_config.pre_tool.filetype.rust]\nbogus = true")]
fn invalid_config_falls_back_to_defaults(#[case] toml_str: &str) {
    let config = parse_config(toml_str);
    assert!(config.enabled);
    assert_eq!(config.hook_config.pre_tool.resolve("Rust").deny_lines_threshold(), 60);
}

#[test]
fn wrong_type_falls_back_to_defaults() {
    let config = parse_config("[hook_config.pre_tool.default]\ndeny_lines_threshold = \"not a number\"");
    assert_eq!(config.hook_config.pre_tool.resolve("Rust").deny_lines_threshold(), 60);
}
