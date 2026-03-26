//! VFS name constants used by core providers.

//! Well-known VFS node names used across providers.
//!
//! SSOT for string constants that appear in the virtual filesystem tree
//! (e.g., `"symbols"`, `"OVERVIEW.md"`). Centralizing these prevents
//! typo-induced mismatches between providers that produce nodes and
//! providers or templates that reference them by name.

use crate::templates::{HandleBuilder, TemplateEngine};
pub(super) use crate::types::path_conventions::{COMPANION_SUFFIX, companion_name};

/// Subdirectory name inside companion directories for sub-provider content.
pub const SUBDIR_SYMBOLS: &str = "symbols";

/// Name of the overview file in companion directories.
pub const FILE_OVERVIEW: &str = "OVERVIEW.md";

/// Root-level guide file (`@/GUIDE.md`).
pub(super) const FILE_GUIDE: &str = "GUIDE.md";

/// Root-level mount status file (`@/STATUS.md`).
pub(super) const FILE_MOUNT_STATUS: &str = "STATUS.md";

/// Register core name constants as template globals.
pub fn register_template_globals(engine: &mut TemplateEngine) {
    crate::register_globals!(engine, FILE_OVERVIEW, SUBDIR_SYMBOLS);
}

/// Create a [`HandleBuilder`] with core name globals pre-registered.
///
/// All providers (core and plugin) should use this instead of
/// `HandleBuilder::new()` so their templates can reference well-known
/// names like `FILE_OVERVIEW` and `SUBDIR_SYMBOLS` without manual
/// registration. Plugin layers can call this and then register their
/// own additional globals on the returned builder.
pub fn handle_builder() -> HandleBuilder {
    let mut b = HandleBuilder::new();
    register_template_globals(b.engine_mut());
    b
}
