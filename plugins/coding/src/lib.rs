//! nyne-coding — syntax decomposition, LSP, and code analysis for nyne.
//!
//! This crate provides the "coding" plugin: tree-sitter parsing, LSP
//! integration, edit planning/splicing, and all providers that depend on
//! source-code understanding.

/// Batch edit staging, splice application, and diff-based code actions.
pub(crate) mod edit;

/// LSP client lifecycle, transport, and query abstractions.
pub(crate) mod lsp;

/// FUSE providers that expose decomposed source code and LSP intelligence.
pub(crate) mod providers;

/// Tree-sitter parsing, symbol decomposition, and source analysis.
pub(crate) mod syntax;

/// Plugin configuration types and deserialization.
mod config;

/// Plugin registration and lifecycle implementation.
mod plugin;

/// Consolidated plugin services bundle.
pub(crate) mod services;

/// Shared test utilities and stub contexts.
#[cfg(test)]
pub(crate) mod test_support;
