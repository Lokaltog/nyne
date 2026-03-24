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

pub const FILE_DEFINITION: &str = "DEFINITION.md";
pub const FILE_DECLARATION: &str = "DECLARATION.md";
pub const FILE_TYPE_DEFINITION: &str = "TYPE-DEFINITION.md";
pub const FILE_REFERENCES: &str = "REFERENCES.md";
pub const FILE_IMPLEMENTATION: &str = "IMPLEMENTATION.md";
pub const FILE_CALLERS: &str = "CALLERS.md";
pub const FILE_DEPS: &str = "DEPS.md";
pub const FILE_SUPERTYPES: &str = "SUPERTYPES.md";
pub const FILE_SUBTYPES: &str = "SUBTYPES.md";
pub const FILE_DOC: &str = "DOC.md";

pub const DIR_DEFINITION: &str = "definition";
pub const DIR_DECLARATION: &str = "declaration";
pub const DIR_TYPE_DEFINITION: &str = "type-definition";
pub const DIR_REFERENCES: &str = "references";
pub const DIR_IMPLEMENTATION: &str = "implementation";
pub const DIR_CALLERS: &str = "callers";
pub const DIR_DEPS: &str = "deps";
pub const DIR_SUPERTYPES: &str = "supertypes";
pub const DIR_SUBTYPES: &str = "subtypes";

pub const SUBDIR_EDIT: &str = "edit";
pub const SUBDIR_STAGED: &str = "staged";
pub const FILE_STAGED_DIFF: &str = "staged.diff";

pub const DIR_TODO: &str = "todo";

/// Register coding name constants as template globals.
pub fn register_template_globals(engine: &mut TemplateEngine) {
    engine.add_global("FILE_OVERVIEW", FILE_OVERVIEW);
    engine.add_global("FILE_CALLERS", FILE_CALLERS);
    engine.add_global("FILE_DEPS", FILE_DEPS);
    engine.add_global("FILE_REFERENCES", FILE_REFERENCES);
    engine.add_global("FILE_DIAGNOSTICS", FILE_DIAGNOSTICS);
    engine.add_global("FILE_IMPLEMENTATION", FILE_IMPLEMENTATION);
    engine.add_global("SUBDIR_SYMBOLS", SUBDIR_SYMBOLS);
    engine.add_global("SUBDIR_ACTIONS", SUBDIR_ACTIONS);
    engine.add_global("VFS_SEP", VFS_SEP);
    engine.add_global("VFS_SYMBOLS_SEP", VFS_SYMBOLS_SEP);
}

/// Create a [`HandleBuilder`] with coding name globals pre-registered.
pub fn handle_builder() -> HandleBuilder {
    let mut b = HandleBuilder::new();
    register_template_globals(b.engine_mut());
    b
}
