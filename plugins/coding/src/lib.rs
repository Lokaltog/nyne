//! nyne-coding — syntax decomposition, LSP, and code analysis for nyne.
//!
//! This crate provides the "coding" plugin: tree-sitter parsing, LSP
//! integration, edit planning/splicing, and all providers that depend on
//! source-code understanding.

pub mod edit;
pub mod lsp;
pub mod providers;
pub mod syntax;

mod config;
mod plugin;

#[cfg(test)]
pub(crate) mod test_support;
