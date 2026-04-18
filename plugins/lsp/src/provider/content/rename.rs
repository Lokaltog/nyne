//! LSP-powered symbol and file rename operations.
//!
//! Covers both symbol-level renames (`textDocument/rename`) and file-level
//! renames (`workspace/willRenameFiles`). Each operation implements [`DiffSource`]
//! for preview and can be applied via `mv` on the VFS directory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::{Result, eyre};
use nyne::router::{AffectedFiles, Filesystem, RenameContext, Renameable};
use nyne_diff::{DiffSource, FileEditResult, apply_file_edits};
use nyne_source::split_disambiguator;

use crate::session::edit::resolve_workspace_edit;
use crate::session::handle::{Handle, LspQuery};
use crate::session::uri;

/// Shared rename computation — implements [`DiffSource`] for preview.
///
/// Used by both the preview file (`rename/new_name.diff` via [`DiffCapable`])
/// and the apply path (`mv old@/ new@/` via [`SymbolRename`]).
#[derive(Clone)]
pub struct RenameDiff {
    pub query: LspQuery,
    pub new_name: String,
}

/// Methods for [`RenameDiff`].
impl RenameDiff {
    /// Execute the LSP rename and return resolved file edits.
    fn resolve(&self) -> Result<Vec<FileEditResult>> {
        let fq = self.query.file_query().ok_or_else(|| eyre!(super::LSP_UNAVAILABLE))?;

        let pos = self.query.position();
        let edit = fq.rename(pos.line, pos.character, &self.new_name)?;
        resolve_workspace_edit(&edit, self.query.path_resolver())
    }
}

/// [`DiffSource`] implementation for [`RenameDiff`].
impl DiffSource for RenameDiff {
    /// Compute edits by resolving the LSP rename.
    fn compute_edits(&self) -> Result<Vec<FileEditResult>> { self.resolve() }

    /// Return a header describing the rename.
    fn header_lines(&self) -> Vec<String> { vec![format!("Rename to: {}", self.new_name)] }
}

/// Rename capability for symbol `@/` directory nodes.
///
/// Attached to fragment directory nodes when an LSP server is available.
/// Triggered by `mv file.rs@/symbols/OldName@/ file.rs@/symbols/NewName@/`.
/// The dispatch layer merges this capability onto `SyntaxProvider`'s directory
/// node via the generalized capability merge.
pub struct SymbolRename {
    pub query: LspQuery,
    pub fs: Arc<dyn Filesystem>,
}

/// [`Renameable`] implementation for [`SymbolRename`].
impl Renameable for SymbolRename {
    /// Apply the rename via LSP and patch the diff.
    fn rename(&self, ctx: &RenameContext<'_>) -> Result<AffectedFiles> {
        // Extract the bare directory name, then strip any
        // disambiguator to get the bare symbol name for LSP.
        let dir_name = ctx
            .target
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| eyre!("rename target has no filename"))?;

        let (new_name, _kind) = split_disambiguator(dir_name);

        let diff = RenameDiff {
            query: self.query.clone(),
            new_name: new_name.to_owned(),
        };

        let edits = diff.resolve()?;
        apply_file_edits(&edits, self.fs.as_ref())
    }
}

/// File rename preview — dry-run `workspace/willRenameFiles` as a [`DiffSource`].
///
/// Reading `file.rs@/rename/new_name.rs.diff` triggers this action, which
/// asks the LSP server what import-path updates would be needed if the file
/// were renamed. The unified diff is returned without performing the rename.
#[derive(Clone)]
pub struct FileRenameDiff {
    pub handle: Arc<Handle>,
    pub source_file: PathBuf,
    pub new_filename: String,
}

/// Methods for [`FileRenameDiff`].
impl FileRenameDiff {
    /// Call `workspace/willRenameFiles` and resolve the workspace edit.
    fn resolve(&self) -> Result<Vec<FileEditResult>> {
        let source_root = self.handle.path_resolver().source_root();
        let old_path = source_root.join(&self.source_file);

        let parent = self.source_file.parent().unwrap_or_else(|| Path::new(""));
        let new_rel = parent.join(&self.new_filename);
        let new_path = source_root.join(&new_rel);

        let old_uri = uri::file_path_to_uri_string(&old_path)?;
        let new_uri = uri::file_path_to_uri_string(&new_path)?;

        let edit = self.handle.client().will_rename_files(&old_uri, &new_uri)?;
        match edit {
            Some(ws_edit) => resolve_workspace_edit(&ws_edit, self.handle.path_resolver()),
            None => Ok(Vec::new()),
        }
    }
}

/// [`DiffSource`] implementation for [`FileRenameDiff`].
impl DiffSource for FileRenameDiff {
    /// Compute edits by resolving the file rename.
    fn compute_edits(&self) -> Result<Vec<FileEditResult>> { self.resolve() }

    /// Return a header describing the file rename operation.
    fn header_lines(&self) -> Vec<String> {
        let old_name = self.source_file.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        vec![format!("Rename file: {old_name} → {}", self.new_filename)]
    }
}
