//! VFS convention helpers for plugin authors.
//!
//! Companion directory creation, dispatch utilities, and path helpers
//! shared across all plugins.
//!
//! During the transition, most helpers delegate to `providers/mod.rs`.
//! They'll move here fully when providers are extracted to plugin crates.

// Re-export path conventions from types — these are part of the public API.
// Re-export provider helpers that plugin crates will need.
pub use crate::providers::{
    companion_children, companion_dir, companion_lookup, companion_symbol_path, dispatch_children, dispatch_lookup,
    is_file_companion, source_file,
};
pub use crate::types::path_conventions::{
    COMPANION_SUFFIX, CompanionSplit, split_companion_path, strip_companion_suffix,
};
