//! Post-tool-use hook — fires after tool execution completes.
//!
//! Decomposed into narrow, single-concern scripts. Claude Code's hook
//! matcher pre-filters each entry by tool name, so every script only
//! sees the tool events it cares about. See
//! [`HOOK_REGISTRY`](crate::provider::hooks::HOOK_REGISTRY) for the
//! full mapping of scripts → matchers.

pub(in crate::provider) mod bash_hints;
pub(in crate::provider) mod cli_alts;
pub(in crate::provider) mod diagnostics;
pub(in crate::provider) mod ssot;
pub(in crate::provider) mod vfs_reread;

#[cfg(feature = "analysis")]
mod analysis;

#[cfg(test)]
mod tests;
