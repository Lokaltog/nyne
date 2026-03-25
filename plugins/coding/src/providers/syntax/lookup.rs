use std::sync::Arc;

use color_eyre::eyre::eyre;
use nyne::dispatch::context::RequestContext;
use nyne::node::VirtualNode;
use nyne::provider::Node;
use nyne::types::vfs_path::VfsPath;

use super::SyntaxProvider;
use super::content::{delete, rename};
use super::resolve::fragment_body_path;
use crate::edit::diff_action::DiffActionNode;
use crate::lsp::handle::LspHandle;
use crate::providers::names::{SUBDIR_AT_LINE, SUBDIR_SYMBOLS};
use crate::services::CodingServices;
use crate::syntax::{find_fragment, find_nearest_fragment_at_line};

/// Symbol lookup methods for [`SyntaxProvider`].
impl SyntaxProvider {
    /// Lookup-only: `symbols/Foo.rs` → symlink to `Foo@/body.rs`.
    pub(super) fn lookup_symbol_shorthand(&self, source_file: &VfsPath, name: &str, _ctx: &RequestContext<'_>) -> Node {
        let Some(decomposer) = self.decomposer_for(source_file) else {
            return Ok(None);
        };
        let ext = decomposer.file_extension();
        // Must match `<fs_name>.<ext>` pattern.
        let suffix = format!(".{ext}");
        let Some(stem) = name.strip_suffix(&suffix) else {
            return Ok(None);
        };
        let shared = CodingServices::get(&self.ctx).decomposition.get(source_file)?;
        // Verify the fragment actually exists at the top level.
        let exists = shared.decomposed.iter().any(|f| f.fs_name.as_deref() == Some(stem));
        if !exists {
            return Ok(None);
        }
        let base = VfsPath::new(SUBDIR_SYMBOLS)?;
        let target = fragment_body_path(&[stem], ext);
        Ok(Some(VirtualNode::symlink(name, target.relative_to(&base))))
    }

    /// Generate a symbol rename preview diff via LSP.
    pub(super) fn lookup_rename_preview_impl(
        &self,
        source_file: &VfsPath,
        fragment_path: &[String],
        name: &str,
        _ctx: &RequestContext<'_>,
    ) -> Node {
        let Some(new_name) = name.strip_suffix(".diff") else {
            return Ok(None);
        };
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Ok(None);
        }

        let Some(_decomposer) = self.decomposer_for(source_file) else {
            return Ok(None);
        };
        let shared = CodingServices::get(&self.ctx).decomposition.get(source_file)?;
        let Some(frag) = find_fragment(&shared.decomposed, fragment_path) else {
            return Ok(None);
        };

        let Some(lsp_handle) = LspHandle::for_file(&self.ctx, source_file) else {
            return Ok(None);
        };

        let query = lsp_handle.at(&shared.source, frag.name_byte_offset);
        let action = rename::RenameDiff {
            query,
            new_name: new_name.to_owned(),
        };
        Ok(Some(VirtualNode::file(name, DiffActionNode::new(name, action))))
    }

    /// Lookup-only: `file.rs@/rename/new_name.rs.diff` → file rename preview.
    ///
    /// Dry-run `workspace/willRenameFiles` — returns a unified diff of all
    /// import-path updates without performing the rename.
    /// Generate a file rename preview diff via LSP willRenameFiles.
    pub(super) fn lookup_file_rename_preview_impl(&self, source_file: &VfsPath, name: &str) -> Node {
        let Some(new_filename) = name.strip_suffix(".diff") else {
            return Ok(None);
        };
        let new_filename = new_filename.trim();
        if new_filename.is_empty() {
            return Ok(None);
        }

        if self.decomposer_for(source_file).is_none() {
            return Ok(None);
        }

        // Validate the new filename forms a valid path (fail-fast at lookup).
        let parent = source_file.parent().unwrap_or(VfsPath::root());
        parent.join(new_filename)?;

        let Some(handle) = LspHandle::for_file(&self.ctx, source_file) else {
            return Ok(None);
        };

        let action = rename::FileRenameDiff {
            handle,
            source_file: source_file.clone(),
            new_filename: new_filename.to_owned(),
        };
        Ok(Some(VirtualNode::file(name, DiffActionNode::new(name, action))))
    }

    /// Generate a delete preview diff for a symbol.
    pub(super) fn lookup_delete_preview(
        &self,
        source_file: &VfsPath,
        fragment_path: &[String],
        _ctx: &RequestContext<'_>,
    ) -> Node {
        let Some(_decomposer) = self.decomposer_for(source_file) else {
            return Ok(None);
        };
        let shared = CodingServices::get(&self.ctx).decomposition.get(source_file)?;
        let Some(_frag) = find_fragment(&shared.decomposed, fragment_path) else {
            return Ok(None);
        };

        let action = delete::SymbolDelete {
            ctx: Arc::clone(&self.ctx),
            source_file: source_file.clone(),
            fragment_path: fragment_path.to_vec(),
        };
        Ok(Some(DiffActionNode::into_node("delete.diff", action)))
    }

    /// Lookup-only: `symbols/at-line/<N>` → symlink to the narrowest symbol
    /// whose line range contains line N (1-based).
    ///
    /// Falls back to the nearest fragment when the line is in a gap
    /// (imports, blank lines between items).
    /// Resolve a line number to a symlink targeting the narrowest symbol.
    pub(super) fn lookup_at_line_impl(&self, source_file: &VfsPath, name: &str, _ctx: &RequestContext<'_>) -> Node {
        let line: usize = name
            .parse()
            .ok()
            .filter(|&n| n > 0)
            .ok_or_else(|| eyre!("at-line: expected positive integer, got {name:?}"))?;
        let Some(decomposer) = self.decomposer_for(source_file) else {
            return Ok(None);
        };
        let shared = CodingServices::get(&self.ctx).decomposition.get(source_file)?;
        let ext = decomposer.file_extension();

        // at-line/ uses 1-based lines; fragment functions use 0-based.
        let Some(frag_path) = find_nearest_fragment_at_line(&shared.decomposed, line - 1, &shared.source) else {
            return Ok(None);
        };

        let base = VfsPath::new(&format!("{SUBDIR_SYMBOLS}/{SUBDIR_AT_LINE}"))?;
        let target = fragment_body_path(&frag_path, ext);
        Ok(Some(VirtualNode::symlink(name, target.relative_to(&base))))
    }
}
