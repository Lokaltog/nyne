//! nyne-source — syntax decomposition and code analysis for nyne.
//!
//! This crate provides the "source" plugin: tree-sitter parsing,
//! edit planning/splicing, and all providers that depend on
//! source-code understanding.

/// Batch edit staging and diff-based code actions.
pub mod edit;

/// FUSE providers that expose decomposed source code.
pub mod providers;

/// Tree-sitter parsing, symbol decomposition, and source analysis.
pub mod syntax;

// Public re-exports for downstream plugin crates.
pub use providers::util::parse_tag_suffix;
pub use providers::well_known::SUBDIR_AT_LINE;
pub use syntax::SyntaxRegistry;
pub use syntax::parser::TsNode;
pub use syntax::spec::Decomposer;

/// Plugin configuration types and deserialization.
mod config;

/// Plugin registration and lifecycle implementation.
mod plugin;

/// Consolidated plugin services bundle.
pub mod services;

/// Shared test utilities and stub contexts.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
