//! Provider implementations that depend on code analysis.
//!
//! Each submodule registers a [`Provider`](nyne::router::Provider) that contributes
//! virtual nodes to the nyne VFS. Providers are activated during plugin startup and
//! expose capabilities ranging from syntax decomposition and batch editing to
//! workspace-wide symbol search.
//!
//! Shared imports are re-exported via [`prelude`]; the remaining modules are
//! [`fragment_resolver`]; the remaining modules are self-contained providers.

/// Syntax decomposition provider — tree-sitter parsing, symbol resolution, and content access.
pub mod syntax;
