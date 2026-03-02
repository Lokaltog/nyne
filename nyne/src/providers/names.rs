//! VFS name constants used by core providers.
//!
//! Plugin-specific names live in their respective crates:
//! - Git names → `nyne_git::names`
//! - Coding names → `nyne_coding::providers::names`

use crate::templates::{HandleBuilder, TemplateEngine};
pub(super) use crate::types::path_conventions::COMPANION_SUFFIX;

/// Subdirectory name inside companion directories for syntax fragments.
pub(super) const SUBDIR_SYMBOLS: &str = "symbols";

/// Name of the overview file in companion directories.
pub(super) const FILE_OVERVIEW: &str = "OVERVIEW.md";

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
