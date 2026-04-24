//! Symbol directory resolution — inventory, fragments, and LSP links.
//!
//! Decomposes source files via tree-sitter and builds the VFS directory tree
//! (`symbols/`, `by-kind/`, per-fragment `@/` directories) that agents navigate.

/// Fragment resolution for symbol directory lookups.
mod fragments;
/// Symbol inventory resolution (symbols root and by-kind filtering).
mod inventory;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::router::{NamedNode, Node};
use nyne_companion::Companion;

use super::SyntaxProvider;
use super::content::FragmentResolver;
use crate::syntax::decomposed::DecomposedSource;
use crate::syntax::find_fragment;
use crate::syntax::fragment::{Fragment, FragmentKind};

/// Pre-fetched decomposition state shared across resolve methods.
///
/// Eliminates the repeated 3-4 line boilerplate of checking for a decomposer,
/// fetching the cached decomposition, and extracting the file extension.
pub(super) struct DecompositionContext {
    pub(super) shared: Arc<DecomposedSource>,
    pub(super) ext: &'static str,
}

impl DecompositionContext {
    /// Look up a fragment by path within the cached decomposition.
    pub(super) fn find_fragment<'a>(&'a self, path: &[String]) -> Option<&'a Fragment> {
        find_fragment(&self.shared.decomposed, path)
    }
}

/// Decomposition context construction and resolver factory.
impl SyntaxProvider {
    /// Build a decomposition context for a source file, returning `None` if unsupported.
    pub(super) fn decomposition_context(&self, source_file: &Path) -> Result<Option<DecompositionContext>> {
        let Some(decomposer) = self.decomposer_for(source_file) else {
            return Ok(None);
        };
        let shared = self.decomposition.get(source_file)?;
        let ext = decomposer.file_extension();
        Ok(Some(DecompositionContext { shared, ext }))
    }

    /// Build a [`FragmentResolver`] for lazy decomposition of a source file.
    pub(super) fn resolver_for(&self, source_file: &Path) -> FragmentResolver {
        FragmentResolver::new(self.decomposition.clone(), source_file.to_path_buf())
    }
}

/// Build virtual nodes for all fragments in a decomposition.
///
/// Each fragment becomes a directory node with a `SymbolLineRange` property
/// and an unlinkable `delete.diff` action.
pub(super) fn build_fragment_nodes(
    provider: &SyntaxProvider,
    companion: &Companion,
    fragments: &[&Fragment],
    source_file: &Path,
    parent_path: &[String],
) -> Vec<NamedNode> {
    fragments
        .iter()
        .filter_map(|frag| {
            let fs_name = frag.fs_name.as_deref()?;
            let dirname = companion.companion_name(fs_name);

            let mut frag_path = parent_path.to_vec();
            frag_path.push(fs_name.to_owned());

            Some(
                Node::dir()
                    .with_unlinkable(provider.delete_unlinkable(source_file, frag_path))
                    .named(dirname),
            )
        })
        .collect()
}

/// Companion root resolution and symlink helpers.
impl SyntaxProvider {
    /// Build the companion-root-relative VFS path to a fragment's body file.
    ///
    /// E.g., `&["Foo", "bar"]` with ext `"rs"` → `symbols/Foo@/bar@/body.rs`.
    pub(in super::super) fn fragment_body_path(
        &self,
        companion: &Companion,
        frag_path: &[impl AsRef<str>],
        ext: &str,
    ) -> PathBuf {
        let mut path = PathBuf::from(&self.vfs.dir.symbols);
        for segment in frag_path {
            path.push(companion.companion_name(segment.as_ref()));
        }
        path.push(format!("{}.{ext}", self.vfs.file.body));
        path
    }
}

/// Derive a file extension from a code block's language tag.
/// Returns `"txt"` for unlabeled blocks.
pub(super) const fn code_block_extension(kind: &FragmentKind) -> &str {
    match kind {
        FragmentKind::CodeBlock { lang: Some(lang) } => lang.as_str(),
        _ => "txt",
    }
}
