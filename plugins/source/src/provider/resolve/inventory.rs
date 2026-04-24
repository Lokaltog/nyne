//! Symbol inventory resolution — `symbols/` root, `by-kind/` filtering,
//! and top-level file nodes (imports, file docstring, OVERVIEW.md).

use std::path::Path;

use color_eyre::eyre::Result;
use nyne::router::{NamedNode, Node};
use nyne_companion::Companion;

use super::build_fragment_nodes;
use crate::provider::SyntaxProvider;
use crate::syntax::fragment::FragmentKind;

/// Symbol inventory methods for [`SyntaxProvider`].
impl SyntaxProvider {
    /// Resolve the `symbols/` root directory, listing top-level fragment dirs.
    ///
    /// Static entries (OVERVIEW.md, imports, docstring, by-kind/) are handled
    /// by the route tree's content and dir entries — this method only emits
    /// the dynamic per-fragment `@`-suffixed directories.
    pub(in super::super) fn resolve_symbols_root(
        &self,
        companion: &Companion,
        source_file: &Path,
    ) -> Result<Option<Vec<NamedNode>>> {
        let Some(dctx) = self.decomposition_context(source_file)? else {
            return Ok(None);
        };

        let top_level: Vec<_> = dctx.shared.decomposed.iter().collect();
        let nodes = build_fragment_nodes(self, companion, &top_level, source_file, &[]);

        Ok(Some(nodes))
    }

    /// Resolve `symbols/by-kind/` — list distinct symbol kinds as directories.
    pub(in super::super) fn resolve_by_kind_root(&self, source_file: &Path) -> Result<Option<Vec<NamedNode>>> {
        let Some(dctx) = self.decomposition_context(source_file)? else {
            return Ok(None);
        };
        let mut kinds: Vec<&str> = dctx
            .shared
            .decomposed
            .iter()
            .filter_map(|f| match &f.kind {
                FragmentKind::Symbol(k) => Some(k.directory_name()),
                _ => None,
            })
            .collect();
        kinds.sort_unstable();
        kinds.dedup();
        let nodes = kinds.into_iter().map(NamedNode::dir).collect();
        Ok(Some(nodes))
    }

    /// Resolve `symbols/by-kind/<kind>/` — symlinks to symbols of that kind.
    pub(in super::super) fn resolve_by_kind_filter(
        &self,
        companion: &Companion,
        source_file: &Path,
        kind_filter: &str,
    ) -> Result<Option<Vec<NamedNode>>> {
        use nyne::path_utils::PathExt;
        let Some(dctx) = self.decomposition_context(source_file)? else {
            return Ok(None);
        };
        let base = Path::new(&self.vfs.dir.symbols)
            .join(&self.vfs.dir.by_kind)
            .join(kind_filter);
        let nodes: Vec<NamedNode> = dctx
            .shared
            .decomposed
            .iter()
            .filter(|f| matches!(&f.kind, FragmentKind::Symbol(k) if k.directory_name() == kind_filter))
            .filter_map(|f| {
                let fs_name = f.fs_name.as_deref()?;
                let link_name = format!("{fs_name}.{}", dctx.ext);
                let target = self.fragment_body_path(companion, &[fs_name], dctx.ext);
                Some(Node::symlink(target.relative_to(&base)).named(link_name))
            })
            .collect();
        if nodes.is_empty() {
            return Ok(None);
        }
        Ok(Some(nodes))
    }
}
