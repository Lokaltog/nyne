//! Default Unix mode bits for backing filesystem metadata.
//!
//! SSOT for values surfaced via [`Metadata::permissions`](super::Metadata)
//! — backends without real mode bits (e.g., [`MemFs`](super::mem::MemFs))
//! and fallback paths when stat fails. Capability-level permissions for
//! virtual nodes live in [`router::node::Permissions`](crate::router::node::Permissions);
//! this module only covers the raw Unix mode bits exposed via the
//! [`Filesystem`](super::Filesystem) trait.
//!
//! The VFS is single-user, so defaults are owner-only.

/// Mask for the permission + setuid portion of a Unix mode (drops file-type bits).
pub const MODE_MASK: u32 = 0o7777;

/// Default mode for a writable regular file (owner-only): `rw-------`.
pub const FILE_DEFAULT: u16 = 0o600;

/// Default mode for a writable directory (owner-only): `rwx------`.
pub const DIR_DEFAULT: u16 = 0o700;

/// Fallback mode for a directory that could not be stat'd (owner-only): `r-x------`.
pub const DIR_FALLBACK: u16 = 0o500;

/// Narrow a raw `u32` mode to `u16` permission bits.
///
/// Applies [`MODE_MASK`] to strip file-type bits and falls back to
/// `default` on overflow — practically unreachable, since 12-bit
/// permission values always fit in `u16`.
pub fn narrow(permissions: u32, default: u16) -> u16 { u16::try_from(permissions & MODE_MASK).unwrap_or(default) }

#[cfg(test)]
mod tests;
