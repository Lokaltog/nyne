//! Provider implementations that depend on code analysis.
//!
//! Each submodule registers a [`Provider`](nyne::provider::Provider) that contributes
//! virtual nodes to the nyne VFS. Providers are activated during plugin startup and
//! expose capabilities ranging from syntax decomposition and batch editing to
//! Claude Code integration and workspace-wide symbol search.
//!
//! Shared utilities live in [`prelude`], [`names`], [`util`], and
//! [`fragment_resolver`]; the remaining modules are self-contained providers.

/// Shared resolver for lazily accessing decomposed source files.
pub mod fragment_resolver;
/// VFS name constants for coding-provided virtual paths.
pub mod names;
/// Common imports re-exported for use across coding providers.
pub mod prelude;
/// Provider utilities for nyne-coding.
pub mod util;

/// Batch editing provider — tracks staged edit operations and applies them atomically.
pub mod batch;
/// Claude Code integration — hooks, settings, and tool dispatch.
pub mod claude;
/// Symbol-scoped git features — per-symbol blame, log, and history.
#[cfg(feature = "git-symbols")]
pub mod git_symbols_companion;
/// Syntax decomposition provider — tree-sitter parsing, symbol resolution, and content access.
pub mod syntax;
/// TODO/FIXME provider — scans source files for TODO markers.
pub mod todo;
/// Workspace symbol search provider — exposes `@/search/symbols/{query}`.
pub mod workspace_search;
