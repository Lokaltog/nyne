//! Coding plugin configuration types and deserialization.
//!
//! All config lives under `[plugin.coding]` in `config.toml`. The top-level
//! struct is [`CodingConfig`], which nests sub-configs for LSP, analysis,
//! Claude hooks, and tool-use policies. Serde's `deny_unknown_fields` is
//! applied on every struct to catch typos early.

/// LSP configuration types.
pub mod lsp;

/// TODO/FIXME comment aggregation configuration.
pub mod todo;

use std::collections::{HashMap, HashSet};

use nyne::json::deep_merge_non_null;
use serde::{Deserialize, Serialize};

use self::lsp::LspConfig;
use self::todo::TodoConfig;

/// Top-level configuration for the coding plugin.
///
/// Deserialized from the `[plugin.coding]` section of `config.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodingConfig {
    /// LSP configuration.
    #[serde(default)]
    pub lsp: LspConfig,

    /// TODO/FIXME comment aggregation configuration.
    #[serde(default)]
    pub todo: TodoConfig,

    /// Code analysis configuration.
    #[serde(default)]
    pub analysis: AnalysisConfig,

    /// Hook behavior configuration.
    #[serde(default)]
    pub hooks: HookConfig,

    /// Claude Code integration configuration.
    #[serde(default)]
    pub claude: ClaudeConfig,
}

/// Hook behavior configuration.
///
/// ```toml
/// [plugin.coding.hooks.pre_tool.default]
/// deny_lines_threshold = 60
/// include_symbol_table = false
///
/// [plugin.coding.hooks.pre_tool.filetype.markdown]
/// deny_lines_threshold = -1
///
/// [plugin.coding.hooks.stop]
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

/// Claude Code integration configuration.
///
/// Controls whether the `.claude/` virtual directory and individual hooks
/// are emitted. All features default to enabled.
///
/// ```toml
/// [plugin.coding.claude]
/// enabled = false              # disables the entire .claude/ tree
///
/// [plugin.coding.claude.hooks]
/// statusline = false           # disable just the statusline hook
/// stop = false                 # disable just the stop hook
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaudeConfig {
    /// Master toggle — `false` disables the entire `.claude/` directory
    /// and all associated hooks/scripts.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Per-hook toggles.
    #[serde(default)]
    pub hooks: ClaudeHooksToggle,
}

/// Default implementation for `ClaudeConfig`.
impl Default for ClaudeConfig {
    /// Returns the default value.
    fn default() -> Self {
        Self {
            enabled: true,
            hooks: ClaudeHooksToggle::default(),
        }
    }
}

/// Per-hook toggles for the Claude Code integration.
#[allow(clippy::struct_excessive_bools)] // each bool is an independent feature toggle
/// Per-hook toggle switches for Claude Code integration.
///
/// All hooks default to enabled; set individual fields to `false`
/// to disable specific hooks while keeping the rest active.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaudeHooksToggle {
    /// Session-start hook (mount status, project context).
    #[serde(default = "default_true")]
    pub session_start: bool,

    /// Pre-tool-use hook (VFS hints, read guards).
    #[serde(default = "default_true")]
    pub pre_tool_use: bool,

    /// Post-tool-use hook (diagnostics, SSOT checks).
    #[serde(default = "default_true")]
    pub post_tool_use: bool,

    /// Stop hook (SSOT/DRY review on session end).
    #[serde(default = "default_true")]
    pub stop: bool,

    /// Statusline hook (live status bar updates).
    #[serde(default = "default_true")]
    pub statusline: bool,
}

/// Default implementation for `ClaudeHooksToggle`.
impl Default for ClaudeHooksToggle {
    /// Returns the default value.
    fn default() -> Self {
        Self {
            session_start: true,
            pre_tool_use: true,
            post_tool_use: true,
            stop: true,
            statusline: true,
        }
    }
}

/// Stop hook configuration — controls when the SSOT/DRY review fires.
///
/// The hook only triggers when the number of modified source files (after
/// filtering out `ignore_extensions`) meets the `min_files` threshold.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopHookConfig {
    /// Minimum number of qualifying changed files required to trigger the
    /// review. Set to `1` to trigger on every source-code change, or `0` to
    /// always trigger (even for non-code-only sessions).
    #[serde(default = "default_min_files")]
    pub min_files: usize,

    /// File extensions to exclude from the changed-file count.
    /// Matched case-insensitively against the file's extension.
    #[serde(default = "default_ignore_extensions")]
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

/// Configuration for the code analysis engine.
///
/// ```toml
/// [plugin.coding.analysis]
/// enabled = true
/// # Absent or omitted: all rules except noisy defaults (magic-string, magic-number).
/// # Explicit empty: all rules, no exclusions.
/// # rules = []
/// # Specific set: only these rules.
/// # rules = ["deep-nesting", "empty-catch", "unwrap-chain"]
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisConfig {
    /// Global kill switch for all code analysis. Default: `true`.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Which analysis rules to activate.
    ///
    /// - `None` (absent from config) → all rules except default-disabled noisy rules.
    /// - `Some([])` (explicit empty) → all registered rules, no exclusions.
    /// - `Some(set)` → only rules whose `id()` matches an entry.
    ///   Unknown names produce a warning at startup.
    pub rules: Option<HashSet<String>>,
}

/// Default implementation for `AnalysisConfig`.
impl Default for AnalysisConfig {
    /// Returns the default value.
    fn default() -> Self {
        Self {
            enabled: true,
            rules: None,
        }
    }
}

/// Serde default function returning `true`.
const fn default_true() -> bool { true }

/// Deserialization and config loading methods.
impl CodingConfig {
    /// Deserialize from the plugin config section, falling back to defaults.
    pub fn from_plugin_config(section: Option<&serde_json::Value>) -> Self {
        let Some(value) = section else {
            return Self::default();
        };
        serde_json::from_value(value.clone()).unwrap_or_default()
    }
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
        let builtin = PreToolPolicy::builtin_defaults(&lang_key);
        let mut resolved = builtin.merge(&self.default);
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
    /// Uses JSON roundtrip so the merge is structurally generic — adding
    /// fields to `PreToolPolicy` requires zero changes here.
    #[expect(clippy::expect_used, reason = "serde roundtrip on a simple struct is infallible")]
    fn merge(self, over: &Self) -> Self {
        let mut base = serde_json::to_value(&self).expect("PreToolPolicy serializes");
        deep_merge_non_null(
            &mut base,
            &serde_json::to_value(over).expect("PreToolPolicy serializes"),
        );
        serde_json::from_value(base).expect("merged PreToolPolicy deserializes")
    }

    /// Resolved `deny_lines_threshold` with builtin fallback.
    pub fn deny_lines_threshold(&self) -> i64 { self.deny_lines_threshold.unwrap_or(60) }

    /// Resolved `narrow_read_limit` with builtin fallback.
    pub fn narrow_read_limit(&self) -> i64 { self.narrow_read_limit.unwrap_or(80) }
}
/// Unit tests for coding plugin configuration.
#[cfg(test)]
mod tests;
