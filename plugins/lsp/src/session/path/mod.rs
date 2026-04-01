// Path rewriting for LSP URIs.
//
// LSP servers return absolute paths using the overlay root. The daemon uses
// these directly for I/O. For user-facing output (templates, diffs), paths
// are rewritten to the display root (`/code` in sandbox). This module is the
// single source of truth for all display ↔ overlay path conversions.

//! Path rewriting between FUSE display root and overlay storage root.
//!
//! LSP servers run as daemon children and see the overlay filesystem,
//! while users and the VFS agent see FUSE mount paths.
//! [`PathResolver`] is the single source of truth for translating
//! between these two path spaces, preventing FUSE re-entrancy and
//! ensuring LSP responses render with user-visible paths.

use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr};

/// Rewrites paths between the FUSE display root and the source storage root.
///
/// LSP servers run as daemon children and see the source filesystem, while
/// users and VFS output see the FUSE mount path. This resolver is the single
/// source of truth for translating between the two:
///
/// - [`rewrite`](Self::rewrite): FUSE root → source root (for daemon file I/O)
/// - [`rewrite_to_fuse`](Self::rewrite_to_fuse): source root → FUSE root (for user-facing output)
///
/// Without this, LSP responses would contain source paths that are
/// meaningless to the agent, and agent-provided paths would not resolve
/// to files the LSP server can read.
pub struct PathResolver {
    /// FUSE mount path — the path the user and agent see (e.g., `/code`).
    fuse_root: PathBuf,
    /// Source storage path — the path the daemon and LSP servers use for I/O.
    source_root: PathBuf,
}

/// Path translation between FUSE mount paths and source storage paths.
impl PathResolver {
    /// Creates a new path resolver with the given FUSE and source roots.
    pub(crate) const fn new(fuse_root: PathBuf, source_root: PathBuf) -> Self { Self { fuse_root, source_root } }

    /// Rewrite an absolute LSP path to its source equivalent.
    ///
    /// If the path starts with `fuse_root`, the prefix is replaced with
    /// `source_root`. Otherwise the path is returned unchanged (it may
    /// already be `source_root`-based, or outside the project entirely).
    pub(crate) fn rewrite(&self, lsp_path: &str) -> PathBuf {
        Path::new(lsp_path)
            .strip_prefix(&self.fuse_root)
            .map_or_else(|_| PathBuf::from(lsp_path), |rel| self.source_root.join(rel))
    }

    /// Rewrite a source-rooted path to its FUSE (user-facing) equivalent.
    ///
    /// Used by workspace edit processing where absolute FUSE paths are needed
    /// for diff output. Template views and symlink construction should use
    /// `strip_prefix(self.source_root())` directly instead — they only need
    /// project-relative paths.
    pub(crate) fn rewrite_to_fuse(&self, source_path: &Path) -> PathBuf {
        source_path
            .strip_prefix(&self.source_root)
            .map_or_else(|_| source_path.to_path_buf(), |rel| self.fuse_root.join(rel))
    }

    /// Read a file at an LSP path, transparently rewriting through `source_root`.
    pub(crate) fn read_to_string(&self, lsp_path: &str) -> Result<String> {
        let real = self.rewrite(lsp_path);
        fs::read_to_string(&real).wrap_err_with(|| format!("failed to read '{}'", real.display()))
    }

    /// Write content to a file at an LSP path, transparently rewriting through `source_root`.
    pub(crate) fn write_string(&self, lsp_path: &str, content: &str) -> Result<()> {
        let real = self.rewrite(lsp_path);
        fs::write(&real, content).wrap_err_with(|| format!("failed to write '{}'", real.display()))
    }

    /// The original project root (FUSE mount path).
    ///
    /// Use for display purposes and diff output — paths shown to the user
    /// should reference the logical mount path, not the source root.
    pub(crate) fn root(&self) -> &Path { &self.fuse_root }

    /// The source root path (daemon-side I/O).
    pub(crate) fn source_root(&self) -> &Path { &self.source_root }
}

/// Unit tests.
#[cfg(test)]
mod tests;
