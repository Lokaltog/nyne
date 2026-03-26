//! Symbol lookup by shorthand, line number, and rename/delete preview.
//!
//! Handles lookup-only paths that are not listed in readdir — shorthand
//! file symlinks (`Foo.rs`), at-line resolution, and diff preview nodes.

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
    /// Resolve a shorthand symbol file to a symlink: `symbols/Foo.rs` → `Foo@/body.rs`.
    ///
    /// Allows direct reads without navigating the `@/` directory layer. Only
    /// matches the `<fs_name>.<ext>` pattern for top-level fragments.
    /// Lookup-only — not listed in readdir.
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

    /// Resolve a symbol rename preview: `Foo@/rename/new_name.diff`.
    ///
    /// Calls the LSP `textDocument/rename` to produce a unified diff of all
    /// changes across the workspace. The actual LSP call happens lazily at
    /// read time through [`DiffActionNode`].
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

        if self.decomposer_for(source_file).is_none() {
            return Ok(None);
        }
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
        Ok(Some(DiffActionNode::into_node(name, action)))
    }

    /// Resolve a file rename preview: `file.rs@/rename/new_name.rs.diff`.
    ///
    /// Dry-runs the LSP `workspace/willRenameFiles` request to produce a unified
    /// diff of all import-path updates that would result from the rename, without
    /// actually performing it. Lookup-only — not listed in readdir.
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
        Ok(Some(DiffActionNode::into_node(name, action)))
    }

    /// Resolve `Foo@/delete.diff` — a preview of removing the symbol from source.
    ///
    /// Produces a unified diff showing the symbol's removal including surrounding
    /// whitespace cleanup. The computation happens lazily at read time.
    pub(super) fn lookup_delete_preview(
        &self,
        source_file: &VfsPath,
        fragment_path: &[String],
        _ctx: &RequestContext<'_>,
    ) -> Node {
        if self.decomposer_for(source_file).is_none() {
            return Ok(None);
        }
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

    /// Resolve `symbols/at-line/<N>` to a symlink targeting the narrowest symbol
    /// whose line range contains line N (1-based).
    ///
    /// Falls back to the nearest fragment when the line falls in a gap
    /// (e.g. imports, blank lines between items). Lookup-only — not listed in readdir.
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
