//! Fragment directory resolution — builds the per-symbol `Foo@/` directory
//! with body, meta-files, code blocks, and child symbol entries.

use nyne::prelude::*;

use super::{build_fragment_nodes, code_block_extension};
use crate::providers::syntax::content::{
    BodySplice, FragmentPath, MetaSplice, SourceSlice, SpliceTarget, build_meta_nodes,
};
use crate::providers::syntax::{SyntaxProvider, newline};
use crate::providers::well_known::{FILE_BODY, SUBDIR_CODE};
use crate::syntax::fragment::FragmentKind;

/// Fragment directory resolution methods for [`SyntaxProvider`].
impl SyntaxProvider {
    /// Resolve a fragment directory (`Foo@/`), assembling its full child listing.
    ///
    /// Emits: `body.<ext>`, meta-files (signature, docstring, decorators, OVERVIEW),
    /// child fragments, and a `code/` directory for fenced code blocks.
    pub(in super::super) fn resolve_fragment_dir(
        &self,
        source_file: &VfsPath,
        fragment_path: &[String],
        _ctx: &RequestContext<'_>,
    ) -> Nodes {
        let Some(dctx) = self.decomposition_context(source_file)? else {
            return Ok(None);
        };

        let Some(parent_frag) = dctx.find_fragment(fragment_path) else {
            return Ok(None);
        };

        let mut nodes = Vec::new();

        // Shared resolver — single source of truth for cache + file identity.
        let resolver = self.resolver_for(source_file);

        // The fragment's full definition (decorators + docstring + signature + body).
        // Byte range is resolved lazily at read time by SourceSlice.
        let body_name = format!("{FILE_BODY}.{}", dctx.ext);
        let frag_path = FragmentPath::new(fragment_path);
        let body_node = VirtualNode::file(&body_name, SourceSlice {
            resolver: resolver.clone(),
            target: SpliceTarget::FragmentBody(frag_path.clone()),
        })
        .with_writable(BodySplice {
            meta: MetaSplice {
                resolver: resolver.clone(),
                target: SpliceTarget::FragmentBody(frag_path),
            },
        });
        nodes.push(newline::with_newline_middlewares(body_node));

        // Per-symbol meta-files (signature, docstring, decorators, overview).
        nodes.extend(build_meta_nodes(
            parent_frag,
            dctx.ext,
            &self.overview,
            &resolver,
            fragment_path,
        ));

        // Child section fragments (excluding code blocks — those go in code/).
        let section_children: Vec<_> = parent_frag
            .children
            .iter()
            .filter(|c| !matches!(c.kind, FragmentKind::CodeBlock { .. }))
            .collect();
        nodes.extend(build_fragment_nodes(
            &section_children,
            &dctx.shared.source,
            source_file,
            fragment_path,
            &self.ctx,
            self.fragment_hook.as_deref(),
        ));

        // code/ directory if this fragment has code block children.
        if parent_frag
            .children
            .iter()
            .any(|c| matches!(c.kind, FragmentKind::CodeBlock { .. }))
        {
            nodes.push(VirtualNode::directory(SUBDIR_CODE));
        }

        Ok(Some(nodes))
    }

    /// Resolve `code/` subdirectory under a document section — list fenced code
    /// blocks as editable files with language-derived extensions.
    pub(in super::super) fn resolve_code_block_dir(
        &self,
        source_file: &VfsPath,
        fragment_path: &[String],
        _ctx: &RequestContext<'_>,
    ) -> Nodes {
        let Some(dctx) = self.decomposition_context(source_file)? else {
            return Ok(None);
        };

        let Some(parent_frag) = dctx.find_fragment(fragment_path) else {
            return Ok(None);
        };

        let resolver = self.resolver_for(source_file);
        let parent_path = FragmentPath::new(fragment_path);

        let nodes: Vec<VirtualNode> = parent_frag
            .children
            .iter()
            .filter(|c| matches!(c.kind, FragmentKind::CodeBlock { .. }))
            .filter_map(|cb| {
                let fs_name = cb.fs_name.as_deref()?;
                let ext = code_block_extension(&cb.kind);
                let filename = format!("{fs_name}.{ext}");
                let target = || SpliceTarget::CodeBlockBody {
                    parent_path: parent_path.clone(),
                    fs_name: fs_name.to_owned(),
                };

                let node = VirtualNode::file(&filename, SourceSlice {
                    resolver: resolver.clone(),
                    target: target(),
                })
                .with_writable(BodySplice {
                    meta: MetaSplice {
                        resolver: resolver.clone(),
                        target: target(),
                    },
                });
                Some(newline::with_newline_middlewares(node))
            })
            .collect();

        Ok(Some(nodes))
    }
}
