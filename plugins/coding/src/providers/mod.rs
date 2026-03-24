//! Provider implementations that depend on code analysis.

/// Shared resolver for lazily accessing decomposed source files.
pub(crate) mod fragment_resolver;
/// VFS name constants for coding-provided virtual paths.
pub(crate) mod names;
/// Common imports re-exported for use across coding providers.
pub(crate) mod prelude;
/// Provider utilities for nyne-coding.
pub(crate) mod util;

/// Batch editing provider — tracks staged edit operations and applies them atomically.
pub mod batch;
/// Claude Code integration — hooks, settings, and tool dispatch.
pub mod claude;
/// Symbol-scoped git features — per-symbol blame, log, and history.
#[cfg(feature = "git-symbols")]
pub(crate) mod git_symbols_companion;
/// Syntax decomposition provider — tree-sitter parsing, symbol resolution, and content access.
pub mod syntax;
/// TODO/FIXME provider — scans source files for TODO markers.
pub mod todo;
/// Workspace symbol search provider — exposes `@/search/symbols/{query}`.
pub mod workspace_search;
