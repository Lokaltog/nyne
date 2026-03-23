//! Shared value types and context structs.

pub mod file_kind;
pub mod path_conventions;
pub mod real_fs;
pub mod slice;
pub mod type_map;
pub mod vfs_path;

mod process_visibility;
mod symbol_line_range;

pub use file_kind::FileKind;
pub use process_visibility::ProcessVisibility;
pub use real_fs::{OsFs, RealFs};
pub use symbol_line_range::{SymbolLineRange, line_of_byte};
pub use type_map::TypeMap;
pub use vfs_path::VfsPath;

/// File extension frequency counts.
///
/// A generic container for `(extension, count)` pairs sorted by frequency
/// (descending). Any plugin can populate this — e.g., a git plugin counts
/// extensions from the index, a filesystem plugin counts from a directory walk.
///
/// Stored in [`ActivationContext`](crate::dispatch::activation::ActivationContext)
/// via [`TypeMap`].
#[derive(Debug, Clone, Default)]
pub struct ExtensionCounts(pub Vec<(String, usize)>);

/// VFS-relative name of the git metadata directory (usually `.git`).
///
/// Inserted into the [`TypeMap`] by the git plugin during activation. Core
/// infrastructure (path filter, watcher) reads this to exclude the git
/// directory from VFS listings and inotify watches.
///
/// Core never imports the git plugin — it only reads this core-defined type.
#[derive(Debug, Clone)]
pub struct GitDirName(pub Option<String>);

/// Additional passthrough process names contributed by plugins at activation time.
///
/// Inserted into the [`TypeMap`] by plugins (e.g., the coding plugin adds LSP
/// server commands). Core merges these with the config-defined
/// [`passthrough_processes`](crate::config::NyneConfig::passthrough_processes)
/// when building the FUSE handler.
///
/// Core never imports plugin crates — it only reads this core-defined type.
#[derive(Debug, Clone, Default)]
pub struct PassthroughProcesses(pub Vec<String>);
