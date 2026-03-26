//! Typed representation of the Claude Code statusline JSON payload.
//!
//! Field names match the Claude Code JSON schema exactly — suppress
//! struct-field-name lints at module level rather than per-struct.

#![allow(clippy::struct_field_names)]

use serde::Deserialize;

/// Top-level payload piped to the statusline script via stdin.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // Wire type — fields exist in the JSON schema but may not be read yet.
/// Parsed statusline JSON payload.
///
/// Mirrors the Claude Code statusline JSON schema. All fields are optional
/// because Claude Code may omit sections depending on subscription tier
/// and session state (e.g., `rate_limits` is only present for Pro/Max).
pub(super) struct StatuslinePayload {
    pub model: Option<Model>,
    pub context_window: Option<ContextWindow>,
    pub cost: Option<Cost>,
    pub rate_limits: Option<RateLimits>,
    pub vim: Option<Vim>,
    #[serde(default)]
    pub exceeds_200k_tokens: bool,
}

#[derive(Debug, Clone, Deserialize)]
/// Model information.
pub(super) struct Model {
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
/// Context window usage details.
pub(super) struct ContextWindow {
    pub context_window_size: Option<u64>,
    #[allow(dead_code)] // Wire field: present in Claude Code's JSON payload
    pub used_percentage: Option<u8>,
    pub current_usage: Option<CurrentUsage>,
}

#[derive(Debug, Clone, Deserialize)]
/// Current token usage.
pub(super) struct CurrentUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
}

/// Methods for [`CurrentUsage`].
impl CurrentUsage {
    /// Total tokens consumed (input + output + cache).
    pub fn total(&self) -> u64 {
        self.input_tokens.unwrap_or(0)
            + self.output_tokens.unwrap_or(0)
            + self.cache_creation_input_tokens.unwrap_or(0)
            + self.cache_read_input_tokens.unwrap_or(0)
    }
}

#[derive(Debug, Clone, Deserialize)]
/// Cost information.
pub(super) struct Cost {
    pub total_lines_added: Option<u64>,
    pub total_lines_removed: Option<u64>,
}
#[derive(Debug, Clone, Deserialize)]
/// Rate limit windows (Pro/Max subscribers only).
pub(super) struct RateLimits {
    pub seven_day: Option<RateWindow>,
}

#[derive(Debug, Clone, Deserialize)]
/// A single rate-limit window.
pub(super) struct RateWindow {
    pub used_percentage: Option<f64>,
    /// Unix epoch (seconds) when this window resets.
    pub resets_at: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
/// Vim editor state.
pub(super) struct Vim {
    pub mode: Option<VimMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
/// Vim editing mode.
pub(super) enum VimMode {
    Normal,
    Insert,
    Visual,
    Replace,
}
