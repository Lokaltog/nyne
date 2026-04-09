//! Pre-tool-use hook — intercepts tool calls before execution.
//!
//! Decomposed into narrow, single-concern scripts. Claude Code's hook
//! matcher pre-filters each entry by tool name, so every script only
//! sees the tool events it cares about. See
//! [`HOOK_REGISTRY`](crate::provider::hooks::HOOK_REGISTRY) for the
//! full mapping of scripts → matchers.

pub(in crate::provider) mod file_access;
pub(in crate::provider) mod grep_symbol;

#[cfg(test)]
mod tests;
