//! nyne-claude — Claude Code integration for nyne.
//!
//! This crate provides the "claude" plugin: hooks, settings, skills,
//! and system prompt injection for Claude Code agent sessions.

/// Claude Code provider — hooks, settings, and tool dispatch.
pub(crate) mod provider;

/// Plugin configuration types.
pub(crate) mod config;

/// Plugin registration and lifecycle implementation.
mod plugin;
