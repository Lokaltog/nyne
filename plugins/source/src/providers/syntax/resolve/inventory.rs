use nyne::prelude::*;

use super::{build_fragment_nodes, fragment_body_path};
use crate::providers::names::{FILE_DOCSTRING, FILE_IMPORTS, FILE_OVERVIEW, SUBDIR_BY_KIND, SUBDIR_SYMBOLS};
use crate::providers::syntax::content::{
    FileDocstringContent, FileDocstringSplice, MetaSplice, OverviewContent, SourceSlice, SpliceTarget,
};
use crate::providers::syntax::{SyntaxProvider, newline};
use crate::syntax::fragment::{FragmentKind, find_fragment_of_kind};

/// Symbol inventory methods for [`SyntaxProvider`].
impl SyntaxProvider {
    /// Resolve the `symbols/` root directory, listing all top-level symbols.
    ///
    /// Emits: OVERVIEW.md, imports file, file-level docstring, `by-kind/`
    /// directory, and one `@`-suffixed directory per top-level fragment.
    pub(in super::super) fn resolve_symbols_root(&self, source_file: &VfsPath, _ctx: &RequestContext<'_>) -> Nodes {
        let Some(dctx) = self.decomposition_context(source_file)? else {
            return Ok(None);
        };
        let filename = source_file.name().unwrap_or("unknown");

        let mut nodes = Vec::new();

        let resolver = self.resolver_for(source_file);

        // OVERVIEW.md
        nodes.push(self.overview.node(FILE_OVERVIEW, OverviewContent {
            resolver: resolver.clone(),
            filename: filename.to_owned(),
        }));

        // Imports file (if present). Byte range resolved lazily by SourceSlice.
        if find_fragment_of_kind(&dctx.shared.decomposed, &FragmentKind::Imports).is_some() {
            let name = format!("{FILE_IMPORTS}.{}", dctx.ext);
            let node = VirtualNode::file(name, SourceSlice {
                resolver: resolver.clone(),
                target: SpliceTarget::Imports,
            })
            .with_writable(MetaSplice {
                resolver: resolver.clone(),
                target: SpliceTarget::Imports,
            });
            nodes.push(newline::with_newline_middlewares(node));
        }

        // File-level docstring (if present).
        if find_fragment_of_kind(&dctx.shared.decomposed, &FragmentKind::Docstring).is_some() {
            let node = VirtualNode::file(FILE_DOCSTRING, FileDocstringContent {
                resolver: resolver.clone(),
            })
            .with_writable(FileDocstringSplice {
                meta: MetaSplice {
                    resolver,
                    target: SpliceTarget::FileDoc,
                },
            });
            nodes.push(newline::with_newline_middlewares(node));
        }

        // by-kind/ directory (only if there are symbols).
        if dctx
            .shared
            .decomposed
            .iter()
            .any(|f| matches!(f.kind, FragmentKind::Symbol(_)))
        {
            nodes.push(VirtualNode::directory(SUBDIR_BY_KIND));
        }

        // Top-level fragments — all as @-suffixed directories.
        let top_level: Vec<_> = dctx.shared.decomposed.iter().collect();
        nodes.extend(build_fragment_nodes(
            &top_level,
            &dctx.shared.source,
            source_file,
            &[],
            &self.ctx,
            self.fragment_hook.as_deref(),
        ));

        Ok(Some(nodes))
    }

    /// Resolve `symbols/by-kind/` — list distinct symbol kinds as directories.
    pub(in super::super) fn resolve_by_kind_root(&self, source_file: &VfsPath, _ctx: &RequestContext<'_>) -> Nodes {
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
        let nodes = kinds.into_iter().map(VirtualNode::directory).collect();
        Ok(Some(nodes))
    }

    /// Resolve `symbols/by-kind/<kind>/` — symlinks to symbols of that kind.
    pub(in super::super) fn resolve_by_kind_filter(
        &self,
        source_file: &VfsPath,
        kind_filter: &str,
        _ctx: &RequestContext<'_>,
    ) -> Nodes {
        let Some(dctx) = self.decomposition_context(source_file)? else {
            return Ok(None);
        };
        let nodes: Vec<VirtualNode> = dctx
            .shared
            .decomposed
            .iter()
            .filter(|f| matches!(&f.kind, FragmentKind::Symbol(k) if k.directory_name() == kind_filter))
            .filter_map(|f| {
                let fs_name = f.fs_name.as_deref()?;
                let link_name = format!("{fs_name}.{}", dctx.ext);
                let base = VfsPath::new(&format!("{SUBDIR_SYMBOLS}/{SUBDIR_BY_KIND}/{kind_filter}")).ok()?;
                let target = fragment_body_path(&[fs_name], dctx.ext);
                Some(VirtualNode::symlink(link_name, target.relative_to(&base)))
            })
            .collect();
        if nodes.is_empty() {
            return Ok(None);
        }
        Ok(Some(nodes))
    }
}
