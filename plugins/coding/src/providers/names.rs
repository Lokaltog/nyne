//! VFS name constants for coding-provided virtual paths.

use nyne::templates::{HandleBuilder, TemplateEngine};
// Re-exported from nyne core — shared with core providers.
pub use nyne::types::path_conventions::{COMPANION_SUFFIX, companion_name};
pub use nyne::{FILE_OVERVIEW, SUBDIR_SYMBOLS};

/// VFS path separator (`@/`).
pub const VFS_SEP: &str = "@/";

/// VFS symbols path segment (`@/symbols/`).
pub const VFS_SYMBOLS_SEP: &str = "@/symbols/";

/// Subdirectory for type-filtered symbol views.
pub const SUBDIR_BY_KIND: &str = "by-kind";

/// Subdirectory for line-number-to-symbol lookups.
pub const SUBDIR_AT_LINE: &str = "at-line";

/// Subdirectory for fenced code blocks.
pub const SUBDIR_CODE: &str = "code";

/// Subdirectory for LSP code actions as `.diff` files.
pub const SUBDIR_ACTIONS: &str = "actions";

/// Diagnostics file name.
pub const FILE_DIAGNOSTICS: &str = "DIAGNOSTICS.md";

/// Analysis hints file name.
pub const FILE_HINTS: &str = "HINTS.md";

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

pub const SUBDIR_EDIT: &str = "edit";
pub const SUBDIR_STAGED: &str = "staged";
pub const FILE_STAGED_DIFF: &str = "staged.diff";

pub const DIR_TODO: &str = "todo";

/// Register coding name constants as template globals.
pub fn register_template_globals(engine: &mut TemplateEngine) {
    nyne::register_globals!(
        engine,
        FILE_OVERVIEW,
        FILE_DIAGNOSTICS,
        SUBDIR_SYMBOLS,
        SUBDIR_ACTIONS,
        VFS_SEP,
        VFS_SYMBOLS_SEP,
    );
}

/// Create a [`HandleBuilder`] with coding name globals pre-registered.
pub fn handle_builder() -> HandleBuilder {
    let mut b = HandleBuilder::new();
    register_template_globals(b.engine_mut());
    b
}
