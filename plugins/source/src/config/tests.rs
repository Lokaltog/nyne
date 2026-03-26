use super::*;

/// Verifies that a missing plugin section returns default config.
#[test]
fn missing_plugin_section_returns_defaults() { let _config = SourceConfig::from_plugin_config(None); }
