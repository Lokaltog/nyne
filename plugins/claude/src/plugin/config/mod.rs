//! Claude plugin configuration types and deserialization.

use std::collections::HashMap;

use nyne::config::PluginConfig;
use nyne::toml_merge;
use serde::{Deserialize, Serialize};

/// Top-level configuration for the claude plugin.
///
/// Deserialized from the `[plugin.claude]` section of `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[allow(clippy::struct_field_names)] // hook_config is the natural TOML key
/// Configuration for the Claude Code integration plugin.
///
/// Deserialized from `[plugin.claude]` in the nyne config file.
/// Controls the master enable/disable toggle, per-hook toggles,
/// and hook-specific behavior settings.
pub struct Config {
    /// Master toggle — `false` disables the entire `.claude/` directory
    /// and all associated hooks/scripts.
    pub enabled: bool,

    /// Per-hook toggles.
    pub hooks: HooksToggle,

    /// Hook behavior configuration.
    pub hook_config: HookConfig,
}

/// Defaults: enabled with all hooks active and builtin hook behavior.
impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            hooks: HooksToggle::default(),
            hook_config: HookConfig::default(),
        }
    }
}

impl PluginConfig for Config {}

/// Per-hook toggles for the Claude Code integration.
#[allow(clippy::struct_excessive_bools)] // each bool is an independent feature toggle
/// Per-hook toggle switches for Claude Code integration.
///
/// All hooks default to enabled; set individual fields to `false`
/// to disable specific hooks while keeping the rest active.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HooksToggle {
    /// Session-start hook (mount status, project context).
    pub session_start: bool,

    /// Pre-tool-use hook (VFS hints, read guards).
    pub pre_tool_use: bool,

    /// Post-tool-use hook (diagnostics, SSOT checks).
    pub post_tool_use: bool,

    /// Post-tool-use failure hook (VFS rename ENOENT recovery).
    pub post_tool_use_failure: bool,

    /// Stop hook (SSOT/DRY review on session end).
    pub stop: bool,

    /// Statusline hook (live status bar updates).
    pub statusline: bool,
}

/// Default implementation for `HooksToggle`.
impl Default for HooksToggle {
    /// Returns the default value.
    fn default() -> Self {
        Self {
            session_start: true,
            pre_tool_use: true,
            post_tool_use: true,
            post_tool_use_failure: true,
            stop: true,
            statusline: true,
        }
    }
}

/// Hook behavior configuration.
///
/// ```toml
/// [plugin.claude.hook_config.pre_tool.default]
/// deny_lines_threshold = 60
/// include_symbol_table = false
///
/// [plugin.claude.hook_config.pre_tool.filetype.markdown]
/// deny_lines_threshold = -1
///
/// [plugin.claude.hook_config.stop]
/// min_files = 2
/// ignore_extensions = ["toml", "md", "json", "yaml", "yml", "lock", "txt"]
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookConfig {
    /// Pre-tool-use hook policy.
    #[serde(default)]
    pub pre_tool: PreToolHookConfig,

    /// Stop hook policy.
    #[serde(default)]
    pub stop: StopHookConfig,
}

/// Stop hook configuration — controls when the SSOT/DRY review fires.
///
/// The hook only triggers when the number of modified source files (after
/// filtering out `ignore_extensions`) meets the `min_files` threshold.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StopHookConfig {
    /// Minimum number of qualifying changed files required to trigger the
    /// review. Set to `1` to trigger on every source-code change, or `0` to
    /// always trigger (even for non-code-only sessions).
    pub min_files: usize,

    /// File extensions to exclude from the changed-file count.
    /// Matched case-insensitively against the file's extension.
    pub ignore_extensions: Vec<String>,
}

/// Default implementation for `StopHookConfig`.
impl Default for StopHookConfig {
    /// Returns the default value.
    fn default() -> Self {
        Self {
            min_files: default_min_files(),
            ignore_extensions: default_ignore_extensions(),
        }
    }
}

/// Default minimum number of changed files to trigger the stop hook.
const fn default_min_files() -> usize { 2 }

/// Default file extensions to exclude from the stop hook changed-file count.
fn default_ignore_extensions() -> Vec<String> {
    ["toml", "md", "json", "yaml", "yml", "lock", "txt"]
        .into_iter()
        .map(String::from)
        .collect()
}

/// Pre-tool-use hook configuration with per-filetype overrides.
///
/// Resolution order (each layer overrides the previous):
/// 1. Hardcoded builtin defaults (per language)
/// 2. User `default` policy
/// 3. User `filetype.<lang>` policy
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreToolHookConfig {
    /// Default policy applied to all filetypes before per-filetype overrides.
    #[serde(default)]
    pub default: PreToolPolicy,

    /// Per-filetype overrides, keyed by lowercased language name (e.g. `markdown`, `rust`).
    #[serde(default)]
    pub filetype: HashMap<String, PreToolPolicy>,
}

/// Policy knobs for the pre-tool-use hook.
///
/// All fields are `Option` to support partial-override merging: `None` means
/// "inherit from the next layer down." The resolved policy (after merging all
/// layers) has all fields populated.
///
/// Implements `Serialize` so the resolved policy can be passed directly into
/// minijinja templates — any field added here is immediately available as
/// `{{ config.<field> }}` with zero plumbing changes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreToolPolicy {
    /// Line count above which a broad `Read` is denied (hard block).
    ///
    /// - Positive: deny files with more than this many lines.
    /// - Zero: always deny (force VFS for all files).
    /// - Negative: never deny (hint only, even for large files).
    pub deny_lines_threshold: Option<i64>,

    /// Maximum `limit` parameter value considered a "narrow" (targeted) read.
    /// Narrow reads always produce a hint, never a deny.
    pub narrow_read_limit: Option<i64>,

    /// Whether to inline the symbol table in hook messages.
    pub include_symbol_table: Option<bool>,
}

/// Policy resolution methods for the pre-tool-use hook.
impl PreToolHookConfig {
    /// Resolve the effective policy for a given language.
    ///
    /// Layers (each overrides the previous):
    /// 1. Hardcoded builtin defaults for the language
    /// 2. User `default` policy
    /// 3. User `filetype.<lang>` override
    pub fn resolve(&self, language_name: &str) -> PreToolPolicy {
        let lang_key = language_name.to_ascii_lowercase();
        let mut resolved = PreToolPolicy::builtin_defaults(&lang_key).merge(&self.default);
        if let Some(ft) = self.filetype.get(&lang_key) {
            resolved = resolved.merge(ft);
        }
        resolved
    }
}

/// Builtin defaults, merging, and resolved accessor methods.
impl PreToolPolicy {
    /// Hardcoded builtin defaults per language category.
    ///
    /// Prose/config formats default to hint-only (never deny); code formats
    /// default to deny above 60 lines.
    fn builtin_defaults(lang: &str) -> Self {
        match lang {
            "markdown" | "toml" => Self {
                deny_lines_threshold: Some(-1),
                narrow_read_limit: Some(80),
                include_symbol_table: Some(false),
            },
            _ => Self {
                deny_lines_threshold: Some(60),
                narrow_read_limit: Some(80),
                include_symbol_table: Some(false),
            },
        }
    }

    /// Overlay `over` onto `self`, producing a merged policy.
    ///
    /// Uses TOML roundtrip so the merge is structurally generic — adding
    /// fields to `PreToolPolicy` requires zero changes here. `None` fields
    /// are omitted during TOML serialization, so absent keys in the overlay
    /// naturally preserve the base value.
    #[expect(clippy::expect_used, reason = "serde roundtrip on a simple struct is infallible")]
    fn merge(self, over: &Self) -> Self {
        let mut base = toml::Value::try_from(&self).expect("PreToolPolicy serializes");
        toml_merge::deep_merge(
            &mut base,
            &toml::Value::try_from(over).expect("PreToolPolicy serializes"),
        );
        base.try_into().expect("merged PreToolPolicy deserializes")
    }

    /// Resolved `deny_lines_threshold` with builtin fallback.
    pub fn deny_lines_threshold(&self) -> i64 { self.deny_lines_threshold.unwrap_or(60) }

    /// Resolved `narrow_read_limit` with builtin fallback.
    pub fn narrow_read_limit(&self) -> i64 { self.narrow_read_limit.unwrap_or(80) }
}

#[cfg(test)]
mod tests;
