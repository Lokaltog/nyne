//! VFS name constants used by core providers.

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
fn register_template_globals(engine: &mut TemplateEngine) {
    engine.add_global("FILE_OVERVIEW", FILE_OVERVIEW);
    engine.add_global("SUBDIR_SYMBOLS", SUBDIR_SYMBOLS);
}

/// Create a [`HandleBuilder`] with core name globals pre-registered.
pub(super) fn handle_builder() -> HandleBuilder {
    let mut b = HandleBuilder::new();
    register_template_globals(b.engine_mut());
    b
}
