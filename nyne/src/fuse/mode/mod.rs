//! FUSE mode bit constants and capability → mode translation.
//!
//! The FUSE protocol surfaces permissions as Unix mode bits (`u16`).
//! [`Permissions`] encodes backend-agnostic capabilities; this module
//! is the sole place that translates between them and the bits
//! returned to the kernel via [`fuser::FileAttr`].
//!
//! The VFS is single-user, so only the owner triplet is ever populated.

use crate::router::Permissions;

/// Mode bit set when the READ capability is present (`r--------`, owner-only).
pub(super) const READ_BITS: u16 = 0o400;

/// Mode bit set when the WRITE capability is present (`-w-------`, owner-only).
pub(super) const WRITE_BITS: u16 = 0o200;

/// Mode bit set when the EXECUTE capability is present (`--x------`, owner-only).
pub(super) const EXECUTE_BITS: u16 = 0o100;

/// FUSE-specific extension for [`Permissions`].
///
/// Mirrors [`std::os::unix::fs::PermissionsExt`] — scoped conversion from
/// backend-agnostic capability flags to the `u16` mode bits the FUSE kernel
/// expects in [`fuser::FileAttr::perm`].
pub(super) trait PermissionsExt {
    /// Translate capability flags to FUSE mode bits.
    fn to_mode_bits(self) -> u16;
}

impl PermissionsExt for Permissions {
    fn to_mode_bits(self) -> u16 {
        let mut mode = 0;
        if self.contains(Self::READ) {
            mode |= READ_BITS;
        }
        if self.contains(Self::WRITE) {
            mode |= WRITE_BITS;
        }
        if self.contains(Self::EXECUTE) {
            mode |= EXECUTE_BITS;
        }
        mode
    }
}

#[cfg(test)]
mod tests;
