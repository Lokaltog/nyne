//! Shared value types and context structs.
//!
//! This module centralizes lightweight data types that cross crate boundaries
//! via the activation context's `AnyMap`. Types here belong to the core
//! library so plugins can produce and consume them without depending on each
//! other — the core acts as the neutral schema layer.

/// Filesystem entry kind enum (file, directory, symlink).
pub mod file_kind;
/// Abstraction over real filesystem operations for FUSE passthrough.
///
/// Slice specification parsing for list-like virtual files (`:M`, `:M-N`, `:-N`).
pub mod slice;
/// Line range metadata for symbol directory nodes.
mod symbol_line_range;
/// Timestamp triplet for virtual filesystem nodes.
mod timestamps;
pub use file_kind::FileKind;
pub use symbol_line_range::SymbolLineRange;
pub use timestamps::Timestamps;

/// File extension frequency counts.
///
/// A generic container for `(extension, count)` pairs sorted by frequency
/// (descending). Any plugin can populate this — e.g., a git plugin counts
/// extensions from the index, a filesystem plugin counts from a directory walk.
///
/// Stored in [`ActivationContext`](crate::dispatch::activation::ActivationContext)
/// via the `AnyMap`.
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
