//! Symbol lookup by shorthand, line number, and rename/delete preview.
//!
//! Handles lookup-only paths that are not listed in readdir — shorthand
//! file symlinks (`Foo.rs`), at-line resolution, and diff preview nodes.

use std::path::Path;
use std::sync::Arc;

use color_eyre::eyre::{Result, eyre};
use nyne::path_utils::PathExt;
use nyne::router::{NamedNode, Node, Request};
use nyne_companion::Companion;
use nyne_diff::DiffRequest;

use super::SyntaxProvider;
use super::content::delete;
use crate::syntax::{find_fragment, find_nearest_fragment_at_line};

/// Symbol lookup methods for [`SyntaxProvider`].
impl SyntaxProvider {
    /// Resolve a shorthand symbol file to a symlink: `symbols/Foo.rs` → `Foo@/body.rs`.
    ///
    /// Allows direct reads without navigating the `@/` directory layer. Only
    /// matches the `<fs_name>.<ext>` pattern for top-level fragments.
    /// Lookup-only — not listed in readdir.
    pub(super) fn lookup_symbol_shorthand(
        &self,
        companion: &Companion,
        source_file: &Path,
        name: &str,
    ) -> Result<Option<NamedNode>> {
        let Some(decomposer) = self.decomposer_for(source_file) else {
            return Ok(None);
        };
        let ext = decomposer.file_extension();
        // Must match `<fs_name>.<ext>` pattern.
        let suffix = format!(".{ext}");
        let Some(stem) = name.strip_suffix(&suffix) else {
            return Ok(None);
        };
        let shared = self.decomposition.get(source_file)?;
        // Verify the fragment actually exists at the top level.
        if !shared.decomposed.iter().any(|f| f.fs_name.as_deref() == Some(stem)) {
            return Ok(None);
        }
        let base = Path::new(&self.vfs.dir.symbols);
        let target = self.fragment_body_path(companion, &[stem], ext);
        Ok(Some(Node::symlink(target.relative_to(base)).named(name)))
    }

    /// Set [`DiffCapable`] state for `delete.diff` — the diff middleware
    /// will create the preview node on lookup or apply on remove.
    pub(super) fn set_delete_diff_source(&self, req: &mut Request, source_file: &Path, fragment_path: &[String]) {
        if self.decomposer_for(source_file).is_none() {
            return;
        }
        let Ok(shared) = self.decomposition.get(source_file) else {
            return;
        };
        if find_fragment(&shared.decomposed, fragment_path).is_none() {
            return;
        }

        req.set_diff_source(
            delete::SymbolDelete {
                decomposition: self.decomposition.clone(),
                source_file: source_file.to_path_buf(),
                fragment_path: fragment_path.to_vec(),
            },
            Arc::clone(&self.fs),
        );
    }

    /// Resolve `symbols/at-line/<N>` to a symlink targeting the narrowest symbol
    /// whose line range contains line N (1-based).
    ///
    /// Falls back to the nearest fragment when the line falls in a gap
    /// (e.g. imports, blank lines between items). Lookup-only — not listed in readdir.
    pub(super) fn lookup_at_line_impl(
        &self,
        companion: &Companion,
        source_file: &Path,
        name: &str,
    ) -> Result<Option<NamedNode>> {
        let line: usize = name
            .parse()
            .ok()
            .filter(|&n| n > 0)
            .ok_or_else(|| eyre!("at-line: expected positive integer, got {name:?}"))?;
        let Some(decomposer) = self.decomposer_for(source_file) else {
            return Ok(None);
        };
        let shared = self.decomposition.get(source_file)?;
        let ext = decomposer.file_extension();

        // at-line/ uses 1-based lines; fragment functions use 0-based.
        let Some(frag_path) = find_nearest_fragment_at_line(&shared.decomposed, line - 1, &shared.rope) else {
            return Ok(None);
        };

        let base = Path::new(&self.vfs.dir.symbols).join(&self.vfs.dir.at_line);
        let target = self.fragment_body_path(companion, &frag_path, ext);
        Ok(Some(Node::symlink(target.relative_to(&base)).named(name)))
    }
}
