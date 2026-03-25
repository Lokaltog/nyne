//! Typed serde schemas for Claude Code hook input and output.
//!
//! Claude Code passes JSON to hook scripts via stdin. These types provide
//! typed deserialization with graceful fallback — every field is `Option`
//! so hooks degrade cleanly if the upstream schema evolves.
//!
//! # Architecture
//!
//! - `HookInput` — common fields present in all hook events
//! - Per-tool input structs (`ReadToolInput`, `EditToolInput`, etc.)
//!   parsed on demand via `HookInput::tool_input_as`
//! - `HookOutput` — unified output covering all response shapes
//!   (context hints, permission decisions, blocking decisions)

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tracing::warn;

// Hook input — common fields

/// Common fields present in all Claude Code hook events.
///
/// Tool-specific hooks (`PreToolUse`, `PostToolUse`) additionally carry
/// `tool_name` and `tool_input`. The raw `tool_input` is deserialized
/// on demand into a typed struct via [`Self::tool_input_as`].
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // Wire type — fields populated via Deserialize
/// Hook input payload deserialized from stdin.
pub struct HookInput {
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
    pub cwd: Option<String>,
    pub hook_event_name: Option<String>,

    // Tool-specific (PreToolUse / PostToolUse / PermissionRequest)
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,

    // Agent context
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,

    // Stop hook guard — true when the stop hook itself triggered this event.
    pub stop_hook_active: Option<bool>,
}

/// Methods for [`HookInput`].
impl HookInput {
    /// Parse from stdin bytes, returning `None` on empty input or parse failure.
    pub fn parse(stdin: &[u8]) -> Option<Self> {
        if stdin.is_empty() {
            return None;
        }
        match serde_json::from_slice(stdin) {
            Ok(input) => Some(input),
            Err(e) => {
                warn!(error = %e, "failed to parse hook input JSON");
                None
            }
        }
    }

    /// Deserialize `tool_input` into a typed struct.
    ///
    /// Returns `None` if `tool_input` is absent or doesn't match `T`'s schema.
    /// This is the graceful fallback path — callers should handle `None`
    /// by skipping tool-specific logic rather than erroring.
    pub fn tool_input_as<T: DeserializeOwned>(&self) -> Option<T> {
        self.tool_input.as_ref().and_then(|v| T::deserialize(v).ok())
    }
}

// Per-tool input schemas

/// Input fields for `Read` tool calls.
#[derive(Debug, Deserialize)]
/// Read tool invocation details.
pub struct ReadToolInput {
    pub file_path: Option<String>,
    pub offset: Option<u64>,
    pub limit: Option<u64>,
}

/// Input fields for `Edit` tool calls.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Wire type — fields populated via Deserialize
/// Edit tool invocation details.
pub struct EditToolInput {
    pub file_path: Option<String>,
    pub old_string: Option<String>,
    pub new_string: Option<String>,
    pub replace_all: Option<bool>,
}

/// Input fields for `Write` tool calls.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Wire type — fields populated via Deserialize
/// Write tool invocation details.
pub struct WriteToolInput {
    pub file_path: Option<String>,
    pub content: Option<String>,
}

/// Input fields for `Bash` tool calls.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Wire type — fields populated via Deserialize
/// Bash tool invocation details.
pub struct BashToolInput {
    pub command: Option<String>,
    pub description: Option<String>,
    pub timeout: Option<u64>,
    pub run_in_background: Option<bool>,
}

/// Input fields for `Grep` tool calls.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Wire type — fields populated via Deserialize
/// Grep tool invocation details.
pub struct GrepToolInput {
    pub pattern: Option<String>,
    pub path: Option<String>,
    pub glob: Option<String>,
    pub output_mode: Option<String>,
    #[serde(rename = "-i")]
    pub case_insensitive: Option<bool>,
    pub multiline: Option<bool>,
}

/// Input fields for `Glob` tool calls.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Wire type — not yet consumed
/// Glob tool invocation details.
pub struct GlobToolInput {
    pub pattern: Option<String>,
    pub path: Option<String>,
}

/// Input fields for `WebFetch` tool calls.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Wire type — not yet consumed
/// `WebFetch` tool invocation details.
pub struct WebFetchToolInput {
    pub url: Option<String>,
    pub prompt: Option<String>,
}

/// Input fields for `WebSearch` tool calls.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Wire type — not yet consumed
/// `WebSearch` tool invocation details.
pub struct WebSearchToolInput {
    pub query: Option<String>,
    pub allowed_domains: Option<Vec<String>>,
    pub blocked_domains: Option<Vec<String>>,
}

/// Input fields for `Agent` tool calls.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Wire type — not yet consumed
/// Agent tool invocation details.
pub struct AgentToolInput {
    pub prompt: Option<String>,
    pub description: Option<String>,
    pub subagent_type: Option<String>,
    pub model: Option<String>,
}

// Hook output

/// Unified hook output covering all response shapes.
///
/// Constructed via [`HookOutput::context`], [`HookOutput::deny`], or
/// [`HookOutput::block`] — each builder sets the appropriate combination
/// of fields for the hook event type.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// Hook output payload serialized to stdout.
pub struct HookOutput {
    /// Blocking decision for `Stop`/`UserPromptSubmit` hooks.
    #[serde(skip_serializing_if = "Option::is_none")]
    decision: Option<&'static str>,

    /// Reason for the blocking decision.
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,

    /// Hook-specific output payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    hook_specific_output: Option<HookSpecificOutput>,
}

/// Inner payload of [`HookOutput`] — event-specific fields.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// Hook-specific output variants.
pub struct HookSpecificOutput {
    /// The hook event name (e.g., `"PreToolUse"`, `"PostToolUse"`).
    hook_event_name: &'static str,

    /// Free-text context appended to the conversation.
    #[serde(skip_serializing_if = "Option::is_none")]
    additional_context: Option<String>,

    /// Permission decision: `"allow"` or `"deny"` (`PreToolUse` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    permission_decision: Option<&'static str>,

    /// Reason shown to the user when denying (`PreToolUse` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    permission_decision_reason: Option<String>,
}

/// Methods for [`HookOutput`].
impl HookOutput {
    /// Emit additional context without affecting tool execution.
    ///
    /// Used by most hooks to surface hints, nudges, and reminders.
    pub const fn context(event: &'static str, message: String) -> Self {
        Self {
            decision: None,
            reason: None,
            hook_specific_output: Some(HookSpecificOutput {
                hook_event_name: event,
                additional_context: Some(message),
                permission_decision: None,
                permission_decision_reason: None,
            }),
        }
    }

    /// Deny a `PreToolUse` tool call with a reason message.
    ///
    /// The tool call is blocked and the reason is shown to the model
    /// as both `additionalContext` and `permissionDecisionReason`.
    pub const fn deny(message: String) -> Self {
        Self {
            decision: None,
            reason: None,
            hook_specific_output: Some(HookSpecificOutput {
                hook_event_name: "PreToolUse",
                additional_context: None,
                permission_decision: Some("deny"),
                permission_decision_reason: Some(message),
            }),
        }
    }

    /// Block a `Stop` or `UserPromptSubmit` event with a reason.
    pub const fn block(reason: String) -> Self {
        Self {
            decision: Some("block"),
            reason: Some(reason),
            hook_specific_output: None,
        }
    }

    /// Serialize to JSON bytes for script output.
    pub fn to_bytes(&self) -> Vec<u8> {
        // Serialization of a well-formed HookOutput should never fail.
        serde_json::to_vec(self).expect("HookOutput serialization is infallible")
    }

    /// Empty output — no effect on the hook event.
    pub const fn empty() -> Vec<u8> { Vec::new() }
}
