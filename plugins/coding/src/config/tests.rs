use std::collections::HashMap;

use rstest::rstest;

use super::*;

fn parse_coding_config(toml_str: &str) -> CodingConfig {
    let toml: toml::Value = toml::from_str(toml_str).unwrap();
    CodingConfig::from_plugin_table(&HashMap::from([("coding".into(), toml)]))
}

fn load_fixture(name: &str) -> CodingConfig {
    let path = format!("{}/src/config/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let content = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"));
    parse_coding_config(&content)
}

#[test]
fn default_has_no_rules_override() {
    let config = AnalysisConfig::default();
    assert!(config.enabled);
    assert!(config.rules.is_none());
}

#[test]
fn missing_plugin_section_returns_defaults() {
    let config = CodingConfig::from_plugin_table(&HashMap::new());
    assert!(config.analysis.enabled);
    assert!(config.analysis.rules.is_none());
}

#[test]
fn absent_rules_key_is_none() {
    let config = parse_coding_config(
        r#"
        [analysis]
        enabled = true
        "#,
    );
    assert!(config.analysis.rules.is_none());
}

#[test]
fn explicit_empty_rules_is_some_empty() {
    let config = parse_coding_config(
        r#"
        [analysis]
        rules = []
        "#,
    );
    assert_eq!(config.analysis.rules, Some(HashSet::new()));
}

#[test]
fn parses_analysis_rules() {
    let config = parse_coding_config(
        r#"
        [analysis]
        rules = ["deep-nesting", "magic-number"]
        "#,
    );
    let rules = config.analysis.rules.unwrap();
    assert_eq!(rules.len(), 2);
    assert!(rules.contains("deep-nesting"));
    assert!(rules.contains("magic-number"));
}

#[test]
fn parses_disabled_analysis() {
    let config = parse_coding_config(
        r#"
        [analysis]
        enabled = false
        "#,
    );
    assert!(!config.analysis.enabled);
}

#[test]
fn rejects_unknown_analysis_fields() {
    let config = parse_coding_config(
        r#"
        [analysis]
        bogus_field = true
        "#,
    );
    // Unknown field should fall back to defaults (unwrap_or_default).
    assert!(config.analysis.enabled);
}

#[rstest]
#[case("Rust", 60)]
#[case("Python", 60)]
#[case("TypeScript", 60)]
#[case("Markdown", -1)]
#[case("TOML", -1)]
fn builtin_deny_threshold(#[case] lang: &str, #[case] expected: i64) {
    let config = PreToolHookConfig::default();
    assert_eq!(config.resolve(lang).deny_lines_threshold(), expected);
}

#[rstest]
#[case("Rust")]
#[case("Markdown")]
fn builtin_narrow_read_limit(#[case] lang: &str) {
    let config = PreToolHookConfig::default();
    assert_eq!(config.resolve(lang).narrow_read_limit(), 80);
}

#[rstest]
#[case("Rust")]
#[case("Markdown")]
fn builtin_no_symbol_table(#[case] lang: &str) {
    let config = PreToolHookConfig::default();
    assert!(!config.resolve(lang).include_symbol_table.unwrap());
}

#[test]
fn builtin_defaults_case_insensitive() {
    let config = PreToolHookConfig::default();
    // language_name() returns "Markdown", config keys are lowercased internally.
    assert_eq!(config.resolve("MARKDOWN").deny_lines_threshold(), -1);
    assert_eq!(config.resolve("markdown").deny_lines_threshold(), -1);
    assert_eq!(config.resolve("Markdown").deny_lines_threshold(), -1);
}

#[test]
fn merge_null_preserves_base() {
    let base = PreToolPolicy {
        deny_lines_threshold: Some(42),
        narrow_read_limit: Some(100),
        include_symbol_table: Some(true),
    };
    let merged = base.merge(&PreToolPolicy::default());
    assert_eq!(merged.deny_lines_threshold, Some(42));
    assert_eq!(merged.narrow_read_limit, Some(100));
    assert_eq!(merged.include_symbol_table, Some(true));
}

#[test]
fn merge_overwrites_non_null() {
    let base = PreToolPolicy {
        deny_lines_threshold: Some(60),
        narrow_read_limit: Some(80),
        include_symbol_table: Some(false),
    };
    let over = PreToolPolicy {
        deny_lines_threshold: Some(-1),
        include_symbol_table: Some(true),
        ..Default::default()
    };
    let merged = base.merge(&over);
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
    // User default overrides builtin for both code and prose.
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
    let config = PreToolHookConfig {
        filetype: HashMap::from([("rust".into(), PreToolPolicy {
            include_symbol_table: Some(true),
            ..Default::default()
        })]),
        ..Default::default()
    };
    let policy = config.resolve("Rust");
    assert!(policy.include_symbol_table.unwrap());
    // Inherited from builtin.
    assert_eq!(policy.deny_lines_threshold(), 60);
    assert_eq!(policy.narrow_read_limit(), 80);
}

#[test]
fn config_from_fixture() {
    let config = load_fixture("pre_tool_overrides.toml");

    // Global defaults from fixture.
    assert_eq!(config.hooks.pre_tool.default.deny_lines_threshold, Some(100));
    assert_eq!(config.hooks.pre_tool.default.include_symbol_table, Some(true));

    // Markdown filetype override.
    let md = &config.hooks.pre_tool.filetype["markdown"];
    assert_eq!(md.deny_lines_threshold, Some(-1));
    assert!(md.include_symbol_table.is_none()); // not set → inherits

    // Rust filetype override.
    let rs = &config.hooks.pre_tool.filetype["rust"];
    assert_eq!(rs.include_symbol_table, Some(false));
    assert!(rs.deny_lines_threshold.is_none()); // not set → inherits

    // Full resolution: markdown gets deny=-1 from filetype, symbol_table=true from default.
    let md_resolved = config.hooks.pre_tool.resolve("Markdown");
    assert_eq!(md_resolved.deny_lines_threshold(), -1);
    assert!(md_resolved.include_symbol_table.unwrap());

    // Full resolution: rust gets deny=100 from default, symbol_table=false from filetype.
    let rs_resolved = config.hooks.pre_tool.resolve("Rust");
    assert_eq!(rs_resolved.deny_lines_threshold(), 100);
    assert!(!rs_resolved.include_symbol_table.unwrap());
}

#[test]
fn absent_hooks_uses_defaults() {
    let config = CodingConfig::from_plugin_table(&HashMap::new());
    assert_eq!(config.hooks.pre_tool.resolve("Rust").deny_lines_threshold(), 60);
    assert_eq!(config.hooks.pre_tool.resolve("Markdown").deny_lines_threshold(), -1);
}

#[rstest]
#[case("[hooks]\nbogus = true", "unknown field in hooks section")]
#[case("[hooks.pre_tool]\nbogus = true", "unknown field in pre_tool section")]
#[case("[hooks.pre_tool.filetype.rust]\nbogus = true", "unknown field in filetype policy")]
fn invalid_config_falls_back_to_defaults(#[case] toml_str: &str, #[case] _desc: &str) {
    // deny_unknown_fields causes deser failure → unwrap_or_default kicks in.
    let config = parse_coding_config(toml_str);
    assert!(config.analysis.enabled);
    assert_eq!(config.hooks.pre_tool.resolve("Rust").deny_lines_threshold(), 60);
}

#[test]
fn wrong_type_falls_back_to_defaults() {
    // deny_lines_threshold should be i64, not string.
    let config = parse_coding_config(
        r#"
        [hooks.pre_tool.default]
        deny_lines_threshold = "not a number"
        "#,
    );
    // Entire CodingConfig falls back to default.
    assert_eq!(config.hooks.pre_tool.resolve("Rust").deny_lines_threshold(), 60);
}

#[test]
fn stop_defaults() {
    let config = CodingConfig::from_plugin_table(&HashMap::new());
    assert_eq!(config.hooks.stop.min_files, 2);
    assert!(config.hooks.stop.ignore_extensions.contains(&"toml".to_owned()));
    assert!(config.hooks.stop.ignore_extensions.contains(&"md".to_owned()));
    assert!(config.hooks.stop.ignore_extensions.contains(&"json".to_owned()));
    assert!(!config.hooks.stop.ignore_extensions.contains(&"rs".to_owned()));
}

#[test]
fn stop_config_from_fixture() {
    let config = load_fixture("stop_overrides.toml");
    assert_eq!(config.hooks.stop.min_files, 3);
    assert_eq!(config.hooks.stop.ignore_extensions, vec!["toml", "md"]);
}

#[test]
fn stop_unknown_field_falls_back_to_defaults() {
    let config = parse_coding_config("[hooks.stop]\nbogus = true");
    assert_eq!(config.hooks.stop.min_files, 2);
}

#[test]
fn claude_defaults_all_enabled() {
    let config = CodingConfig::from_plugin_table(&HashMap::new());
    assert!(config.claude.enabled);
    assert!(config.claude.hooks.session_start);
    assert!(config.claude.hooks.pre_tool_use);
    assert!(config.claude.hooks.post_tool_use);
    assert!(config.claude.hooks.stop);
    assert!(config.claude.hooks.statusline);
}

#[test]
fn claude_disabled() {
    let config = parse_coding_config("[claude]\nenabled = false");
    assert!(!config.claude.enabled);
    // Hook toggles still default to true even when master is off.
    assert!(config.claude.hooks.session_start);
}

#[test]
fn claude_individual_hooks_disabled() {
    let config = parse_coding_config("[claude.hooks]\nstatusline = false\nstop = false");
    assert!(config.claude.enabled);
    assert!(config.claude.hooks.session_start);
    assert!(config.claude.hooks.pre_tool_use);
    assert!(config.claude.hooks.post_tool_use);
    assert!(!config.claude.hooks.stop);
    assert!(!config.claude.hooks.statusline);
}

#[test]
fn claude_unknown_field_falls_back_to_defaults() {
    let config = parse_coding_config("[claude]\nbogus = true");
    assert!(config.claude.enabled);
}

#[test]
fn claude_hooks_unknown_field_falls_back_to_defaults() {
    let config = parse_coding_config("[claude.hooks]\nbogus = true");
    assert!(config.claude.hooks.session_start);
}

#[test]
fn claude_config_from_fixture() {
    let config = load_fixture("claude_overrides.toml");
    assert!(config.claude.enabled);
    assert!(!config.claude.hooks.statusline);
    assert!(!config.claude.hooks.stop);
    assert!(config.claude.hooks.pre_tool_use);
    assert!(config.claude.hooks.post_tool_use);
    assert!(config.claude.hooks.session_start);
}
