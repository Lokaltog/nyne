//! Typed representation of the Claude Code statusline JSON payload.
//!
//! Field names match the Claude Code JSON schema exactly — suppress
//! struct-field-name lints at module level rather than per-struct.

#![allow(clippy::struct_field_names)]

use serde::Deserialize;

/// Parsed statusline JSON payload piped to the statusline script via stdin.
///
/// Mirrors the Claude Code statusline JSON schema. All fields are optional
/// because Claude Code may omit sections depending on subscription tier
/// and session state (e.g., `rate_limits` is only present for Pro/Max).
#[allow(dead_code)] // Wire type — fields exist in the JSON schema but may not be read yet.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct StatuslinePayload {
    pub model: Option<Model>,
    pub context_window: Option<ContextWindow>,
    pub cost: Option<Cost>,
    pub rate_limits: Option<RateLimits>,
    pub vim: Option<Vim>,
    #[serde(default)]
    pub exceeds_200k_tokens: bool,
}

/// Model information.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct Model {
    pub display_name: Option<String>,
}

/// Context window usage details.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct ContextWindow {
    pub context_window_size: Option<u64>,
    #[allow(dead_code)] // Wire field: present in Claude Code's JSON payload
    pub used_percentage: Option<u8>,
    pub current_usage: Option<CurrentUsage>,
}

/// Current token usage.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct CurrentUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
}

impl CurrentUsage {
    /// Total tokens consumed (input + output + cache).
    pub fn total(&self) -> u64 {
        self.input_tokens.unwrap_or(0)
            + self.output_tokens.unwrap_or(0)
            + self.cache_creation_input_tokens.unwrap_or(0)
            + self.cache_read_input_tokens.unwrap_or(0)
    }
}

/// Cost information.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct Cost {
    pub total_lines_added: Option<u64>,
    pub total_lines_removed: Option<u64>,
}

/// Rate limit windows (Pro/Max subscribers only).
#[derive(Debug, Clone, Deserialize)]
pub(super) struct RateLimits {
    pub seven_day: Option<RateWindow>,
}

/// A single rate-limit window.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct RateWindow {
    pub used_percentage: Option<f64>,
    /// Unix epoch (seconds) when this window resets.
    pub resets_at: Option<u64>,
}

/// Vim editor state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(super) struct Vim {
    pub mode: Option<VimMode>,
}

/// Vim editing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub(super) enum VimMode {
    Normal,
    Insert,
    Visual,
    Replace,
}
