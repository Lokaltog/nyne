use rstest::rstest;

use super::*;

/// Parses a TOML string into a `CodingConfig` for testing.
fn parse_coding_config(toml_str: &str) -> CodingConfig {
    let value: serde_json::Value = toml::from_str(toml_str).unwrap();
    CodingConfig::from_plugin_config(Some(&value))
}

/// Loads a `CodingConfig` from a named test fixture file.
fn load_fixture(name: &str) -> CodingConfig {
    let path = format!("{}/src/config/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let content = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"));
    parse_coding_config(&content)
}

/// Verifies that AnalysisConfig defaults have no rules override.
#[test]
fn default_has_no_rules_override() {
    let config = AnalysisConfig::default();
    assert!(config.enabled);
    assert!(config.rules.is_none());
}

/// Verifies that a missing plugin section returns default config.
#[test]
fn missing_plugin_section_returns_defaults() {
    let config = CodingConfig::from_plugin_config(None);
    assert!(config.analysis.enabled);
    assert!(config.analysis.rules.is_none());
}

/// Verifies that absent rules key deserializes as None.
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

/// Verifies that an explicit empty rules array deserializes as Some(empty).
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

/// Verifies that specific analysis rules are parsed correctly.
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

/// Verifies that disabled analysis is parsed correctly.
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

/// Verifies that unknown analysis fields trigger fallback to defaults.
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

/// Verifies builtin deny_lines_threshold per language category.
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

/// Verifies builtin narrow_read_limit across languages.
#[rstest]
#[case("Rust")]
#[case("Markdown")]
fn builtin_narrow_read_limit(#[case] lang: &str) {
    let config = PreToolHookConfig::default();
    assert_eq!(config.resolve(lang).narrow_read_limit(), 80);
}

/// Verifies that builtin defaults disable symbol table inclusion.
#[rstest]
#[case("Rust")]
#[case("Markdown")]
fn builtin_no_symbol_table(#[case] lang: &str) {
    let config = PreToolHookConfig::default();
    assert!(!config.resolve(lang).include_symbol_table.unwrap());
}

/// Verifies that builtin language matching is case-insensitive.
#[test]
fn builtin_defaults_case_insensitive() {
    let config = PreToolHookConfig::default();
    // language_name() returns "Markdown", config keys are lowercased internally.
    assert_eq!(config.resolve("MARKDOWN").deny_lines_threshold(), -1);
    assert_eq!(config.resolve("markdown").deny_lines_threshold(), -1);
    assert_eq!(config.resolve("Markdown").deny_lines_threshold(), -1);
}

/// Verifies that merging a null overlay preserves base values.
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

/// Verifies that non-null overlay values overwrite base values.
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

/// Verifies that user default policy overrides builtin defaults.
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

/// Verifies that filetype policy overrides user default policy.
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

/// Verifies that partial filetype config inherits unset fields from base.
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

/// Verifies full config resolution from a fixture with multiple overrides.
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

/// Verifies that absent hooks section uses default policy values.
#[test]
fn absent_hooks_uses_defaults() {
    let config = CodingConfig::from_plugin_config(None);
    assert_eq!(config.hooks.pre_tool.resolve("Rust").deny_lines_threshold(), 60);
    assert_eq!(config.hooks.pre_tool.resolve("Markdown").deny_lines_threshold(), -1);
}

/// Verifies that invalid config sections fall back to defaults.
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

/// Verifies that wrong-typed config values fall back to defaults.
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

/// Verifies stop hook default values.
#[test]
fn stop_defaults() {
    let config = CodingConfig::from_plugin_config(None);
    assert_eq!(config.hooks.stop.min_files, 2);
    assert!(config.hooks.stop.ignore_extensions.contains(&"toml".to_owned()));
    assert!(config.hooks.stop.ignore_extensions.contains(&"md".to_owned()));
    assert!(config.hooks.stop.ignore_extensions.contains(&"json".to_owned()));
    assert!(!config.hooks.stop.ignore_extensions.contains(&"rs".to_owned()));
}

/// Verifies stop hook config loaded from a fixture.
#[test]
fn stop_config_from_fixture() {
    let config = load_fixture("stop_overrides.toml");
    assert_eq!(config.hooks.stop.min_files, 3);
    assert_eq!(config.hooks.stop.ignore_extensions, vec!["toml", "md"]);
}

/// Verifies that unknown stop hook fields fall back to defaults.
#[test]
fn stop_unknown_field_falls_back_to_defaults() {
    let config = parse_coding_config("[hooks.stop]\nbogus = true");
    assert_eq!(config.hooks.stop.min_files, 2);
}

/// Verifies that all Claude hooks default to enabled.
#[test]
fn claude_defaults_all_enabled() {
    let config = CodingConfig::from_plugin_config(None);
    assert!(config.claude.enabled);
    assert!(config.claude.hooks.session_start);
    assert!(config.claude.hooks.pre_tool_use);
    assert!(config.claude.hooks.post_tool_use);
    assert!(config.claude.hooks.stop);
    assert!(config.claude.hooks.statusline);
}

/// Verifies that setting claude enabled=false disables the master toggle.
#[test]
fn claude_disabled() {
    let config = parse_coding_config("[claude]\nenabled = false");
    assert!(!config.claude.enabled);
    // Hook toggles still default to true even when master is off.
    assert!(config.claude.hooks.session_start);
}

/// Verifies that individual Claude hooks can be disabled independently.
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

/// Verifies that unknown Claude config fields fall back to defaults.
#[test]
fn claude_unknown_field_falls_back_to_defaults() {
    let config = parse_coding_config("[claude]\nbogus = true");
    assert!(config.claude.enabled);
}

/// Verifies that unknown Claude hooks fields fall back to defaults.
#[test]
fn claude_hooks_unknown_field_falls_back_to_defaults() {
    let config = parse_coding_config("[claude.hooks]\nbogus = true");
    assert!(config.claude.hooks.session_start);
}

/// Verifies Claude config loaded from a fixture with partial overrides.
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

/// Helper: find a server entry by name.
fn find_server<'a>(config: &'a CodingConfig, name: &str) -> &'a lsp::ServerEntry {
    config
        .lsp
        .servers
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("server '{name}' not found"))
}

/// Verifies LSP global config fields from various fixtures.
#[rstest]
#[case::defaults_when_omitted("lsp_empty_section.toml", true, 300, 2)]
#[case::custom_durations("lsp_custom_durations.toml", true, 600, 5)]
fn lsp_global_config(
    #[case] fixture: &str,
    #[case] enabled: bool,
    #[case] cache_ttl_secs: u64,
    #[case] diag_timeout_secs: u64,
) {
    let config = load_fixture(fixture);
    assert_eq!(config.lsp.enabled, enabled);
    assert_eq!(config.lsp.cache_ttl, std::time::Duration::from_secs(cache_ttl_secs));
    assert_eq!(
        config.lsp.diagnostics_timeout,
        std::time::Duration::from_secs(diag_timeout_secs)
    );
}

/// Verifies that LSP can be explicitly disabled.
#[test]
fn lsp_disabled() {
    let config = load_fixture("lsp_disabled.toml");
    assert!(!config.lsp.enabled);
}

/// Verifies per-server field overrides from fixtures.
#[rstest]
#[case::override_args(
    "lsp_server_override_args.toml", "rust-analyzer",
    true, None, Some(&["--log-file", "/tmp/ra.log"][..])
)]
#[case::override_command(
    "lsp_server_override_command.toml",
    "pyright",
    true,
    Some("basedpyright-langserver"),
    None
)]
#[case::disable("lsp_server_disable.toml", "basedpyright", false, None, None)]
fn lsp_server_overrides(
    #[case] fixture: &str,
    #[case] server_name: &str,
    #[case] enabled: bool,
    #[case] command: Option<&str>,
    #[case] args: Option<&[&str]>,
) {
    let config = load_fixture(fixture);
    let server = find_server(&config, server_name);
    assert_eq!(server.enabled, enabled);
    if let Some(cmd) = command {
        assert_eq!(server.command.as_deref(), Some(cmd));
    }
    if let Some(expected_args) = args {
        let actual: Vec<&str> = server.args.as_ref().unwrap().iter().map(String::as_str).collect();
        assert_eq!(actual.as_slice(), expected_args);
    }
}

/// Verifies custom server entries are present and enabled.
#[rstest]
#[case::single("lsp_custom_server.toml", &["ruff"])]
#[case::multiple("lsp_multiple_custom_servers.toml", &["ruff", "deno"])]
fn lsp_custom_servers(#[case] fixture: &str, #[case] expected_names: &[&str]) {
    let config = load_fixture(fixture);
    for name in expected_names {
        assert!(
            find_server(&config, name).enabled,
            "server '{name}' should be present and enabled"
        );
    }
}

/// Verifies that custom servers can omit args.
#[test]
fn lsp_custom_server_no_args() {
    let config = load_fixture("lsp_custom_server_no_args.toml");
    let gopls = find_server(&config, "gopls");
    assert!(gopls.args.is_none());
}

/// Verifies a complete LSP configuration with all field types.
#[test]
fn lsp_full_config() {
    let config = load_fixture("lsp_full.toml");
    assert!(config.lsp.enabled);
    assert_eq!(config.lsp.cache_ttl, std::time::Duration::from_secs(600));
    assert_eq!(config.lsp.diagnostics_timeout, std::time::Duration::from_secs(3));
    assert_eq!(config.lsp.servers.len(), 3);
    assert!(!find_server(&config, "basedpyright").enabled);
    assert!(find_server(&config, "ruff").enabled);
}

/// Verifies that invalid config sections are rejected by serde.
#[rstest]
#[case::unknown_lsp("[lsp]\nbogus = true")]
#[case::unknown_server("[[lsp.servers]]\nname = \"foo\"\nbogus = true")]
#[case::unknown_todo("[todo]\nbogus = true")]
fn reject_invalid_config(#[case] toml_input: &str) {
    let result: std::result::Result<CodingConfig, _> = toml::from_str(toml_input);
    assert!(result.is_err(), "invalid config should be rejected: {toml_input}");
}

/// Verifies LanguageIdMapping deserialization and resolve.
#[rstest]
#[case::uniform_string(r#"language_ids = "rust""#, "rs", Some("rust"))]
#[case::uniform_any_ext(r#"language_ids = "python""#, "py", Some("python"))]
#[case::per_ext_match(
    r#"language_ids = { ts = "typescript", tsx = "typescriptreact" }"#,
    "ts",
    Some("typescript")
)]
#[case::per_ext_other(
    r#"language_ids = { ts = "typescript", tsx = "typescriptreact" }"#,
    "tsx",
    Some("typescriptreact")
)]
#[case::per_ext_miss(r#"language_ids = { ts = "typescript" }"#, "jsx", None)]
fn language_id_mapping_resolve(#[case] toml_input: &str, #[case] ext: &str, #[case] expected: Option<&str>) {
    #[derive(serde::Deserialize)]
    struct Wrapper {
        language_ids: lsp::LanguageIdMapping,
    }
    let w: Wrapper = toml::from_str(toml_input).unwrap();
    assert_eq!(w.language_ids.resolve(ext), expected);
}

/// Verifies that overlay merges Some fields and preserves None fields.
#[test]
fn server_entry_overlay_merges_fields() {
    let mut base = lsp::ServerEntry {
        name: "test".into(),
        command: Some("old-cmd".into()),
        args: Some(vec!["--old".into()]),
        extensions: Some(vec!["rs".into()]),
        ..Default::default()
    };
    let overlay = lsp::ServerEntry {
        name: "test".into(),
        args: Some(vec!["--new".into()]),
        root_markers: Some(vec!["Cargo.toml".into()]),
        ..Default::default()
    };
    base.overlay(&overlay);

    assert_eq!(base.args.as_deref(), Some(&["--new".into()][..]));
    assert_eq!(base.root_markers.as_deref(), Some(&["Cargo.toml".into()][..]),);
    // Base retains command and extensions (overlay had None)
    assert_eq!(base.command.as_deref(), Some("old-cmd"));
    assert_eq!(base.extensions.as_deref(), Some(&["rs".into()][..]));
}

/// Verifies that overlay always overwrites enabled.
#[test]
fn server_entry_overlay_overwrites_enabled() {
    let mut base = lsp::ServerEntry {
        name: "test".into(),
        ..Default::default()
    };
    let overlay = lsp::ServerEntry {
        name: "test".into(),
        enabled: false,
        ..Default::default()
    };
    base.overlay(&overlay);
    assert!(!base.enabled);
}
