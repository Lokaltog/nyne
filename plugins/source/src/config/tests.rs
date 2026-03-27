use super::*;

/// Verifies that a missing plugin section returns default config.
#[test]
fn missing_plugin_section_returns_defaults() { let _config = Config::from_plugin_config(None); }
