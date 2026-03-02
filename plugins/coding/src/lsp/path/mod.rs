// Path rewriting for LSP URIs.
//
// LSP servers return absolute paths using the overlay root. The daemon uses
// these directly for I/O. For user-facing output (templates, diffs), paths
// are rewritten to the display root (`/code` in sandbox). This module is the
// single source of truth for all display ↔ overlay path conversions.

use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr};

/// Rewrites paths between the display root (`/code`), overlay root
/// (daemon I/O), and LSP server paths.
///
/// - `rewrite`: display root → overlay root (for daemon file I/O)
/// - `rewrite_to_fuse`: overlay root → display root (for user-facing output)
///
/// Single source of truth for all path conversions in the LSP layer.
pub struct LspPathResolver {
    /// Display root — the path the user sees (`/code` in sandbox).
    fuse_root: PathBuf,
    overlay_root: PathBuf,
}

impl LspPathResolver {
    pub(crate) const fn new(fuse_root: PathBuf, overlay_root: PathBuf) -> Self {
        Self {
            fuse_root,
            overlay_root,
        }
    }

    /// Rewrite an absolute LSP path to its overlay equivalent.
    ///
    /// If the path starts with `fuse_root`, the prefix is replaced with
    /// `overlay_root`. Otherwise the path is returned unchanged (it may
    /// already be `overlay_root`-based, or outside the project entirely).
    pub(crate) fn rewrite(&self, lsp_path: &str) -> PathBuf {
        Path::new(lsp_path)
            .strip_prefix(&self.fuse_root)
            .map_or_else(|_| PathBuf::from(lsp_path), |rel| self.overlay_root.join(rel))
    }

    /// Rewrite an overlay-rooted path to its FUSE (user-facing) equivalent.
    ///
    /// Used for paths returned by LSP servers (which run at `overlay_root`)
    /// that need to be displayed to the user or matched against `fuse_root`.
    pub(crate) fn rewrite_to_fuse(&self, overlay_path: &Path) -> PathBuf {
        overlay_path
            .strip_prefix(&self.overlay_root)
            .map_or_else(|_| overlay_path.to_path_buf(), |rel| self.fuse_root.join(rel))
    }

    /// Read a file at an LSP path, transparently rewriting through `overlay_root`.
    pub(crate) fn read_to_string(&self, lsp_path: &str) -> Result<String> {
        let real = self.rewrite(lsp_path);
        fs::read_to_string(&real).wrap_err_with(|| format!("failed to read '{}'", real.display()))
    }

    /// Write content to a file at an LSP path, transparently rewriting through `overlay_root`.
    pub(crate) fn write_string(&self, lsp_path: &str, content: &str) -> Result<()> {
        let real = self.rewrite(lsp_path);
        fs::write(&real, content).wrap_err_with(|| format!("failed to write '{}'", real.display()))
    }

    /// The original project root (FUSE mount path).
    ///
    /// Use for display purposes and diff output — paths shown to the user
    /// should reference the logical mount path, not the overlay.
    pub(crate) fn root(&self) -> &Path { &self.fuse_root }

    /// The overlay root path (daemon-side I/O).
    pub(crate) fn overlay_root(&self) -> &Path { &self.overlay_root }
}

#[cfg(test)]
mod tests;
