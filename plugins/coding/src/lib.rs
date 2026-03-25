//! nyne-coding — syntax decomposition, LSP, and code analysis for nyne.
//!
//! This crate provides the "coding" plugin: tree-sitter parsing, LSP
//! integration, edit planning/splicing, and all providers that depend on
//! source-code understanding.

/// File editing operations: planning, splicing, and diff actions.
pub mod edit;

/// Language Server Protocol client and integration.
pub mod lsp;

/// VFS providers for syntax, Claude hooks, TODO tracking, and search.
pub mod providers;

/// Tree-sitter parsing, decomposition, and syntax registry.
pub mod syntax;

/// Plugin configuration types and deserialization.
mod config;

/// Plugin registration and lifecycle implementation.
mod plugin;

/// Consolidated plugin services bundle.
pub(crate) mod services;

/// Shared test utilities and stub contexts.
#[cfg(test)]
pub(crate) mod test_support;
