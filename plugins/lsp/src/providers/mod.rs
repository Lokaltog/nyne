//! LSP VFS providers -- bridge between LSP intelligence and the virtual filesystem.
//!
//! Two providers live here:
//! - [`provider::LspProvider`] contributes LSP-powered nodes (CALLERS.md, DEPS.md,
//!   REFERENCES.md, rename/, actions/, DIAGNOSTICS.md) to symbol directories owned
//!   by `SyntaxProvider` via multi-provider composition.
//! - [`workspace_search::WorkspaceSearchProvider`] exposes `@/search/symbols/{query}`
//!   for project-wide symbol search via LSP workspace symbols.

pub mod content;
mod lsp_links;
pub(crate) mod provider;
pub mod workspace_search;
