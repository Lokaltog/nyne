//! nyne-coding — syntax decomposition, LSP, and code analysis for nyne.
//!
//! This crate provides the "coding" plugin: tree-sitter parsing, LSP
//! integration, edit planning/splicing, and all providers that depend on
//! source-code understanding.

/// Batch edit staging, splice application, and diff-based code actions.
pub(crate) mod edit;

/// LSP client lifecycle, transport, and query abstractions.
pub mod lsp;

/// FUSE providers that expose decomposed source code and LSP intelligence.
pub mod providers;

/// Tree-sitter parsing, symbol decomposition, and source analysis.
pub mod syntax;

// Public re-exports for downstream plugin crates.
pub use providers::names::SUBDIR_AT_LINE;
pub use providers::util::parse_tag_suffix;
pub use syntax::SyntaxRegistry;
pub use syntax::spec::Decomposer;

/// Plugin configuration types and deserialization.
mod config;

/// Plugin registration and lifecycle implementation.
mod plugin;

/// Consolidated plugin services bundle.
pub mod services;

/// Shared test utilities and stub contexts.
#[cfg(test)]
pub(crate) mod test_support;
