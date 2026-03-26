//! Shared value types and context structs.
//!
//! This module centralizes lightweight data types that cross crate boundaries
//! via [`TypeMap`]. Types here belong to the core library so plugins can
//! produce and consume them without depending on each other — the core acts
//! as the neutral schema layer.

/// Filesystem entry kind enum (file, directory, symlink).
pub mod file_kind;
/// VFS path naming conventions: companion suffix, companion split, fragment parsing.
pub mod path_conventions;
/// Abstraction over real filesystem operations for FUSE passthrough.
pub mod real_fs;
/// Slice specification parsing for list-like virtual files (`:M`, `:M-N`, `:-N`).
pub mod slice;
/// `TypeId`-keyed heterogeneous map for typed property storage.
pub mod type_map;
/// Virtual filesystem relative paths with validation and normalization.
pub mod vfs_path;

/// Per-process visibility levels controlling VFS content filtering.
mod process_visibility;
/// Line range metadata for symbol directory nodes.
mod symbol_line_range;

pub use file_kind::FileKind;
pub use process_visibility::ProcessVisibility;
pub use real_fs::{OsFs, RealFs};
pub use symbol_line_range::SymbolLineRange;
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
pub struct ExtensionCounts(Vec<(String, usize)>);

impl ExtensionCounts {
    /// Wrap a pre-sorted `(extension, count)` list.
    ///
    /// Callers are responsible for sorting by count descending.
    pub const fn new(counts: Vec<(String, usize)>) -> Self { Self(counts) }

    /// Borrow the `(extension, count)` pairs.
    pub fn as_slice(&self) -> &[(String, usize)] { &self.0 }
}

/// VFS-relative name of the git metadata directory (usually `.git`).
///
/// Inserted into the [`TypeMap`] by the git plugin during activation **only
/// when a git directory is detected**. Absence from the `TypeMap` means
/// "no git directory" — no double-option needed.
///
/// Core never imports the git plugin — it only reads this core-defined type.
#[derive(Debug, Clone)]
pub struct GitDirName(pub String);

/// Additional passthrough process names contributed by plugins at activation time.
///
/// Inserted into the [`TypeMap`] by plugins (e.g., the coding plugin adds LSP
/// server commands). Core merges these with the config-defined
/// [`passthrough_processes`](crate::config::NyneConfig::passthrough_processes)
/// when building the FUSE handler.
///
/// Core never imports plugin crates — it only reads this core-defined type.
#[derive(Debug, Clone, Default)]
pub struct PassthroughProcesses(Vec<String>);

impl PassthroughProcesses {
    /// Wrap a list of process names that should receive raw filesystem passthrough.
    pub const fn new(names: Vec<String>) -> Self { Self(names) }

    /// Borrow the process name list.
    pub fn as_slice(&self) -> &[String] { &self.0 }
}
