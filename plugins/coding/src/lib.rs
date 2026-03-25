//! nyne-coding — syntax decomposition, LSP, and code analysis for nyne.
//!
//! This crate provides the "coding" plugin: tree-sitter parsing, LSP
//! integration, edit planning/splicing, and all providers that depend on
//! source-code understanding.

pub(crate) mod edit;

pub(crate) mod lsp;

pub(crate) mod providers;

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
