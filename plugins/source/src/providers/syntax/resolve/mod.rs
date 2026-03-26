//! Symbol directory resolution — inventory, fragments, and LSP links.
//!
//! Decomposes source files via tree-sitter and builds the VFS directory tree
//! (`symbols/`, `by-kind/`, per-fragment `@/` directories) that agents navigate.

/// Fragment resolution for symbol directory lookups.
mod fragments;
/// Symbol inventory resolution (symbols root and by-kind filtering).
mod inventory;
use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::dispatch::activation::ActivationContext;
use nyne::dispatch::context::RequestContext;
use nyne::node::VirtualNode;
use nyne::types::SymbolLineRange;
use nyne::types::vfs_path::VfsPath;

use super::SyntaxProvider;
use super::content::{FileOverviewContent, FragmentResolver, LinesContent, LinesWrite, delete};
use crate::edit::diff_action::DiffActionNode;
use crate::providers::names::{COMPANION_SUFFIX, FILE_BODY, FILE_OVERVIEW, SUBDIR_SYMBOLS, companion_name};
use crate::services::SourceServices;
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

/// Methods for [`DecompositionContext`].
impl DecompositionContext {
    /// Look up a fragment by path within the cached decomposition.
    /// Look up a fragment by path within the cached decomposition.
    pub(super) fn find_fragment<'a>(&'a self, path: &[String]) -> Option<&'a Fragment> {
        find_fragment(&self.shared.decomposed, path)
    }
}

/// Decomposition context construction and resolver factory.
impl SyntaxProvider {
    /// Build a decomposition context for a source file, returning `None` if unsupported.
    pub(super) fn decomposition_context(&self, source_file: &VfsPath) -> Result<Option<DecompositionContext>> {
        let Some(decomposer) = self.decomposer_for(source_file) else {
            return Ok(None);
        };
        let shared = SourceServices::get(&self.ctx).decomposition.get(source_file)?;
        let ext = decomposer.file_extension();
        Ok(Some(DecompositionContext { shared, ext }))
    }

    /// Build a [`FragmentResolver`] for lazy decomposition of a source file.
    pub(super) fn resolver_for(&self, source_file: &VfsPath) -> FragmentResolver {
        let cache = SourceServices::get(&self.ctx).decomposition.clone();
        FragmentResolver::new(cache, source_file.clone())
    }
}

/// Companion root resolution and node building.
impl SyntaxProvider {
    /// Resolve the companion root: emit symbols directory and file-level nodes.
    pub(super) fn resolve_companion_root(
        &self,
        source_file: &VfsPath,
        ctx: &RequestContext<'_>,
    ) -> Option<Vec<VirtualNode>> {
        // Only activate for files (not dirs) with a registered syntax.
        if !ctx.real_fs.exists(source_file) || ctx.real_fs.is_dir(source_file) {
            return None;
        }
        let decomposer = self.decomposer_for(source_file)?;
        let mut nodes = vec![VirtualNode::directory(SUBDIR_SYMBOLS)];

        // File-level OVERVIEW.md — richer view with metadata + symbol table.
        {
            let resolver = self.resolver_for(source_file);
            let filename = source_file.name().unwrap_or("unknown").to_owned();
            let language = decomposer.language_name().to_owned();
            nodes.push(self.file_overview.node(FILE_OVERVIEW, FileOverviewContent {
                resolver,
                filename,
                language,
            }));
        }

        // Note: DIAGNOSTICS.md is intentionally NOT in the resolved listing.
        // It is lookup-only (hidden from readdir) to avoid expensive
        // diagnostic pulls on a bare `ls`. Available via direct access.

        // Bare `lines` — always present for syntax-enabled files.
        // `.sliceable()` enables `lines:M-N` derivation via the LineSlice plugin.
        let lines_node = VirtualNode::file("lines", LinesContent {
            source_file: source_file.clone(),
        })
        .with_writable(LinesWrite {
            source_file: source_file.clone(),
            decomposer: Arc::clone(decomposer),
            resolver: self.resolver_for(source_file),
        })
        .sliceable();
        nodes.push(lines_node);

        Some(nodes)
    }
}

/// Build virtual nodes for all fragments in a decomposition.
///
/// Each fragment becomes a directory node with a `SymbolLineRange` property
/// and an unlinkable `delete.diff` action.
pub(super) fn build_fragment_nodes(
    fragments: &[&Fragment],
    source: &str,
    source_file: &VfsPath,
    parent_path: &[String],
    activation: &Arc<ActivationContext>,
) -> Vec<VirtualNode> {
    fragments
        .iter()
        .filter_map(|frag| {
            let fs_name = frag.fs_name.as_deref()?;
            let dirname = companion_name(fs_name);
            let meta = SymbolLineRange::from_zero_based(&frag.line_range(source));
            let mut node = VirtualNode::directory(dirname).prop(meta);

            let mut frag_path = parent_path.to_vec();
            frag_path.push(fs_name.to_owned());
            node = node.with_unlinkable(DiffActionNode::new("delete.diff", delete::SymbolDelete {
                ctx: Arc::clone(activation),
                source_file: source_file.clone(),
                fragment_path: frag_path,
            }));

            Some(node)
        })
        .collect()
}

/// Build the companion-root-relative VFS path to a fragment's body file.
///
/// E.g., `&["Foo", "bar"]` with ext `"rs"` → `symbols/Foo@/bar@/body.rs`.
#[expect(clippy::expect_used, reason = "constructed from validated constants")]
pub(super) fn fragment_body_path(frag_path: &[impl AsRef<str>], ext: &str) -> VfsPath {
    let mut segments = String::from(SUBDIR_SYMBOLS);
    for segment in frag_path {
        segments.push('/');
        segments.push_str(segment.as_ref());
        segments.push_str(COMPANION_SUFFIX);
    }
    segments.push('/');
    segments.push_str(FILE_BODY);
    segments.push('.');
    segments.push_str(ext);
    // SAFETY: constructed from validated constants — always a valid VfsPath.
    VfsPath::new(&segments).expect("fragment_body_path produced invalid VfsPath")
}

/// Derive a file extension from a code block's language tag.
/// Returns `"txt"` for unlabeled blocks.
pub(super) const fn code_block_extension(kind: &FragmentKind) -> &str {
    match kind {
        FragmentKind::CodeBlock { lang: Some(lang) } => lang.as_str(),
        _ => "txt",
    }
}
