/// File extension frequency counts.
///
/// A generic container for `(extension, count)` pairs sorted by frequency
/// (descending). Any plugin can populate this — e.g., a git plugin counts
/// extensions from the index, a filesystem plugin counts from a directory walk.
///
/// Stored in [`ActivationContext`](crate::dispatch::activation::ActivationContext)
/// via [`TypeMap`](super::TypeMap).
#[derive(Debug, Clone, Default)]
pub struct ExtensionCounts(pub Vec<(String, usize)>);
