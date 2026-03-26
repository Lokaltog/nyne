//! VFS name constants for coding-provided virtual paths.
//!
//! Centralizes every directory name, file name, and separator that the coding
//! plugin injects into the virtual filesystem. Constants defined here are the
//! single source of truth — providers, templates, and path-parsing helpers all
//! reference them rather than duplicating string literals.
//!
//! The module also re-exports shared constants from nyne core
//! ([`FILE_OVERVIEW`], [`SUBDIR_SYMBOLS`], [`COMPANION_SUFFIX`]) so that
//! coding-side code has a single import site.

use nyne::templates::{HandleBuilder, TemplateEngine};
// Re-exported from nyne core — shared with core providers.
pub use nyne::types::path_conventions::{COMPANION_SUFFIX, companion_name};
pub use nyne::{FILE_OVERVIEW, SUBDIR_SYMBOLS};

/// Subdirectory for type-filtered symbol views.
pub const SUBDIR_BY_KIND: &str = "by-kind";

/// Subdirectory for line-number-to-symbol lookups.
pub const SUBDIR_AT_LINE: &str = "at-line";

/// Subdirectory for fenced code blocks.
pub const SUBDIR_CODE: &str = "code";

/// Symbol body file name.
pub const FILE_BODY: &str = "body";

/// Signature meta-file name.
pub const FILE_SIGNATURE: &str = "signature";

/// Docstring meta-file name.
pub const FILE_DOCSTRING: &str = "docstring.txt";

/// Decorators meta-file name.
pub const FILE_DECORATORS: &str = "decorators";

/// Imports file name.
pub const FILE_IMPORTS: &str = "imports";

/// Subdirectory for batch edit staging operations.
pub const SUBDIR_EDIT: &str = "edit";
/// Subdirectory for previewing staged edit actions.
pub const SUBDIR_STAGED: &str = "staged";
/// Staged diff file name — apply on delete, clear on truncate.
///
/// This file has dual semantics: reading it previews the staged diff,
/// deleting it (`rm`) atomically applies all staged edits, and truncating
/// it (writing empty content) discards all staged edits without applying.
pub const FILE_STAGED_DIFF: &str = "staged.diff";

/// Register coding name constants as template globals.
///
/// Calls core's [`nyne::register_template_globals`] first, then adds
/// source-specific constants so templates can reference canonical names
/// (e.g. `{{ FILE_OVERVIEW }}`, `{{ FILE_BODY }}`) without hard-coding
/// string literals.
pub fn register_template_globals(engine: &mut TemplateEngine) { nyne::register_template_globals(engine); }

/// Create a [`HandleBuilder`] with coding name globals pre-registered.
///
/// Chains from [`nyne::handle_builder`] (which registers core globals),
/// then adds source-specific globals via [`register_template_globals`].
/// Every source-layer provider that registers templates should start here.
pub fn handle_builder() -> HandleBuilder {
    let mut b = nyne::handle_builder();
    register_template_globals(b.engine_mut());
    b
}
/// Extract the symbol name from a VFS path like `file.rs@/symbols/Foo@/body.rs`.
///
/// Looks for the `@/symbols/` segment, then returns the next path component
/// (up to but not including the following `@/`).
pub fn symbol_from_vfs_path(path: &str) -> Option<&str> {
    let symbols_sep = concat!("@", "/symbols/");
    let after = path.split(symbols_sep).nth(1)?;
    let name = after.split(nyne::VFS_SEPARATOR).next()?;
    if name.is_empty() { None } else { Some(name) }
}

/// Check whether a path points to a symbols OVERVIEW.md (`@/symbols/` … `OVERVIEW.md`).
pub fn is_symbols_overview(path: &str) -> bool {
    let symbols_sep = concat!("@", "/symbols/");
    path.contains(symbols_sep) && path.ends_with(FILE_OVERVIEW)
}

#[cfg(test)]
mod tests;
