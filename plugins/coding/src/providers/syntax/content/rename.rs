// Rename — DiffAction for preview, Renameable for apply.
// Covers both symbol-level renames (textDocument/rename) and file-level
// renames (workspace/willRenameFiles).

use std::sync::Arc;

use color_eyre::eyre::{Result, eyre};
use nyne::dispatch::context::{RenameContext, RequestContext};
use nyne::helpers::strip_companion_suffix;
use nyne::node::capabilities::Renameable;
use nyne::node::diff_action::DiffAction;
use nyne::types::vfs_path::VfsPath;

use crate::edit::plan::{FileEditResult, apply_file_edits};
use crate::lsp::edit::resolve_workspace_edit;
use crate::lsp::handle::{LspHandle, SymbolQuery};
use crate::lsp::uri;
use crate::syntax::fs_mapping::split_disambiguator;

/// Shared rename computation — implements [`DiffAction`] for preview.
///
/// Used by both the preview file (`rename/new_name.diff` via [`DiffActionNode`])
/// and the apply path (`mv old@/ new@/` via [`SymbolRename`]).
///
/// [`DiffActionNode`]: nyne::node::diff_action::DiffActionNode
#[derive(Clone)]
pub(in crate::providers::syntax) struct RenameDiff {
    pub query: SymbolQuery,
    pub new_name: String,
}

impl RenameDiff {
    /// Execute the LSP rename and return resolved file edits.
    fn resolve(&self) -> Result<Vec<FileEditResult>> {
        let fq = self
            .query
            .file_query()
            .ok_or_else(|| eyre!(super::lsp::LSP_UNAVAILABLE))?;

        let pos = self.query.position();
        let edit = fq.rename(pos.line, pos.character, &self.new_name)?;
        let resolver = self.query.path_resolver();
        resolve_workspace_edit(&edit, resolver)
    }
}

impl DiffAction for RenameDiff {
    fn compute_edits(&self, _ctx: &RequestContext<'_>) -> Result<Vec<FileEditResult>> { self.resolve() }

    fn header_lines(&self) -> Vec<String> { vec![format!("Rename to: {}", self.new_name)] }
}

/// Rename capability for symbol `@/` directory nodes.
///
/// Attached to fragment directory nodes when an LSP server is available.
/// Triggered by `mv file.rs@/symbols/OldName@/ file.rs@/symbols/NewName@/`.
pub(in crate::providers::syntax) struct SymbolRename {
    pub query: SymbolQuery,
}

impl Renameable for SymbolRename {
    fn rename(&self, ctx: &RenameContext<'_>) -> Result<()> {
        // The target_name from FUSE is the new directory name (e.g., "NewName@"
        // or "NewName~Struct@"). Strip the companion suffix, then strip any
        // disambiguator to get the bare symbol name for LSP.
        let dir_name = strip_companion_suffix(ctx.target_name)
            .ok_or_else(|| eyre!("rename target must end with companion suffix"))?;

        let (new_name, _kind) = split_disambiguator(dir_name);

        let diff = RenameDiff {
            query: self.query.clone(),
            new_name: new_name.to_owned(),
        };

        let edits = diff.resolve()?;
        apply_file_edits(&edits, ctx.request.real_fs)?;

        Ok(())
    }
}

/// File rename preview — dry-run `workspace/willRenameFiles` as a [`DiffAction`].
///
/// Reading `file.rs@/rename/new_name.rs.diff` triggers this action, which
/// asks the LSP server what import-path updates would be needed if the file
/// were renamed. The unified diff is returned without performing the rename.
///
/// [`DiffAction`]: nyne::node::diff_action::DiffAction
pub(in crate::providers::syntax) struct FileRenameDiff {
    pub handle: Arc<LspHandle>,
    pub source_file: VfsPath,
    pub new_filename: String,
}

impl FileRenameDiff {
    /// Call `workspace/willRenameFiles` and resolve the workspace edit.
    fn resolve(&self) -> Result<Vec<FileEditResult>> {
        let overlay_root = self.handle.path_resolver().overlay_root();
        let old_path = overlay_root.join(self.source_file.as_str());

        let parent = self.source_file.parent().unwrap_or(VfsPath::root());
        let new_vfs = parent.join(&self.new_filename)?;
        let new_path = overlay_root.join(new_vfs.as_str());

        let old_uri = uri::file_path_to_uri_string(&old_path)?;
        let new_uri = uri::file_path_to_uri_string(&new_path)?;

        let edit = self.handle.client().will_rename_files(&old_uri, &new_uri)?;
        match edit {
            Some(ws_edit) => resolve_workspace_edit(&ws_edit, self.handle.path_resolver()),
            None => Ok(Vec::new()),
        }
    }
}

impl DiffAction for FileRenameDiff {
    fn compute_edits(&self, _ctx: &RequestContext<'_>) -> Result<Vec<FileEditResult>> { self.resolve() }

    fn header_lines(&self) -> Vec<String> {
        let old_name = self.source_file.name().unwrap_or("?");
        vec![format!("Rename file: {old_name} → {}", self.new_filename)]
    }
}
