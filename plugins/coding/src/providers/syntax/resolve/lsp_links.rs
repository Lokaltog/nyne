use std::collections::HashSet;
use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;
use nyne::dispatch::context::RequestContext;
use nyne::node::VirtualNode;
use nyne::provider::Nodes;
use nyne::types::vfs_path::VfsPath;

use crate::lsp::handle::LspHandle;
use crate::providers::names::{COMPANION_SUFFIX, SUBDIR_SYMBOLS, companion_name};
use crate::providers::syntax::SyntaxProvider;
use crate::providers::syntax::content::actions;
use crate::providers::syntax::content::lsp::{LspTarget, query_lsp_targets};
use crate::syntax::decomposed::DecompositionCache;
use crate::syntax::find_fragment_at_line;

/// Build a companion `VfsPath` to a specific symbol in a decomposed target file.
///
/// Returns `None` if the target can't be decomposed or the fragment isn't found,
/// in which case the caller should fall back to a line-slice link.
#[expect(
    clippy::expect_used,
    reason = "programming error if DecompositionCache missing after activation"
)]
fn resolve_symbol_link(
    provider: &SyntaxProvider,
    target_vfs: &VfsPath,
    rel_path: &str,
    target_line: u32,
    base: &VfsPath,
) -> Option<PathBuf> {
    provider.decomposer_for(target_vfs)?;
    let target_shared = provider
        .ctx
        .get::<DecompositionCache>()
        .expect("coding plugin not activated")
        .get(target_vfs)
        .ok()?;
    let frag_path = find_fragment_at_line(&target_shared.decomposed, target_line as usize, &target_shared.source)?;
    let mut to = VfsPath::new(&format!("{}/{SUBDIR_SYMBOLS}", companion_name(rel_path))).ok()?;
    for name in &frag_path {
        to = to.join(&companion_name(name)).ok()?;
    }
    Some(to.relative_to(base))
}

/// Build a fallback line-slice link when symbol resolution fails.
#[expect(
    clippy::expect_used,
    reason = "VfsPath format is known-good — programming error if it fails"
)]
fn fallback_line_link(rel_path: &str, target_line: u32, base: &VfsPath) -> PathBuf {
    let line_1based = target_line + 1;
    let to = VfsPath::new(&format!("{}/lines:{line_1based}", companion_name(rel_path)))
        .expect("lsp link target produced invalid VfsPath");
    to.relative_to(base)
}

/// Build the base `VfsPath` for the symlink directory.
///
/// Layout: `<source_file>@/symbols/<frag1>@/.../<fragN>@/<lsp_dir>`
fn build_symlink_base(source_file: &VfsPath, fragment_path: &[String], lsp_dir: &str) -> Result<VfsPath> {
    let mut base_path = format!("{}@/{SUBDIR_SYMBOLS}", source_file.as_str());
    for frag in fragment_path {
        base_path.push('/');
        base_path.push_str(frag);
        base_path.push_str(COMPANION_SUFFIX);
    }
    base_path.push('/');
    base_path.push_str(lsp_dir);
    VfsPath::new(&base_path)
}

/// Build a display name for an LSP target link.
fn target_link_name(target: &LspTarget) -> String {
    let file_basename = target
        .abs_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let line_1based = target.line + 1;
    match &target.name {
        Some(name) => format!("{name}-{file_basename}:{line_1based}"),
        None => format!("{file_basename}:{line_1based}"),
    }
}

impl SyntaxProvider {
    /// Resolve an LSP symlink directory for a symbol.
    ///
    /// Called for paths like `file.rs@/symbols/Foo@/callers/`.
    /// Queries LSP, then reverse-maps each result to a symbol in the
    /// target file via tree-sitter decomposition.
    pub(in super::super) fn resolve_lsp_symlink_dir(
        &self,
        source_file: &VfsPath,
        fragment_path: &[String],
        lsp_dir: &str,
        _ctx: &RequestContext<'_>,
    ) -> Nodes {
        let Some(dctx) = self.decomposition_context(source_file)? else {
            return Ok(None);
        };

        let Some(frag) = dctx.find_fragment(fragment_path) else {
            return Ok(None);
        };

        let Some(lsp_handle) = LspHandle::for_file(&self.ctx, source_file) else {
            return Ok(None);
        };

        let targets = query_lsp_targets(
            &lsp_handle,
            &dctx.shared.source,
            frag.name_byte_offset,
            lsp_dir,
            &frag.line_range(&dctx.shared.source),
        )?;

        if targets.is_empty() {
            return Ok(Some(Vec::new()));
        }

        let root = self.ctx.root();
        let base = build_symlink_base(source_file, fragment_path, lsp_dir)?;
        let nodes = self.build_target_nodes(&targets, root, &base);
        Ok(Some(nodes))
    }

    /// Convert LSP targets into deduplicated symlink nodes.
    fn build_target_nodes(&self, targets: &[LspTarget], root: &Path, base: &VfsPath) -> Vec<VirtualNode> {
        let mut nodes = Vec::new();
        let mut seen = HashSet::new();

        for target in targets {
            let Some(rel_path) = target.abs_path.strip_prefix(root).ok().and_then(|p| p.to_str()) else {
                continue;
            };
            let Ok(target_vfs) = VfsPath::new(rel_path) else {
                continue;
            };

            let symlink_target = resolve_symbol_link(self, &target_vfs, rel_path, target.line, base)
                .unwrap_or_else(|| fallback_line_link(rel_path, target.line, base));

            let link_name = target_link_name(target);
            if !seen.contains(&link_name) {
                seen.insert(link_name.clone());
                nodes.push(VirtualNode::symlink(link_name, symlink_target));
            }
        }

        nodes
    }

    /// Resolve the `actions/` directory for a symbol.
    ///
    /// Eagerly fetches code actions from the LSP server and builds
    /// `.diff` file nodes for each one.
    pub(in super::super) fn resolve_actions_dir(
        &self,
        source_file: &VfsPath,
        fragment_path: &[String],
        _ctx: &RequestContext<'_>,
    ) -> Nodes {
        let Some(dctx) = self.decomposition_context(source_file)? else {
            return Ok(None);
        };

        let Some(frag) = dctx.find_fragment(fragment_path) else {
            return Ok(None);
        };

        let Some(lsp_handle) = LspHandle::for_file(&self.ctx, source_file) else {
            return Ok(None);
        };

        let sym = lsp_handle.at(&dctx.shared.source, frag.name_byte_offset);
        let resolved = actions::resolve_code_actions(&sym, &frag.line_range(&dctx.shared.source));
        let nodes = actions::build_action_nodes(resolved, &sym);
        Ok(Some(nodes))
    }
}
