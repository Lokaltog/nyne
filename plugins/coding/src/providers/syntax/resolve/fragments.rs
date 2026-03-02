use nyne::dispatch::context::RequestContext;
use nyne::node::VirtualNode;
use nyne::provider::Nodes;
use nyne::types::vfs_path::VfsPath;

use super::{build_fragment_nodes, code_block_extension};
use crate::lsp::handle::LspHandle;
use crate::providers::names::{FILE_BODY, SUBDIR_ACTIONS, SUBDIR_CODE};
use crate::providers::syntax::content::lsp::build_lsp_symbol_nodes;
use crate::providers::syntax::content::{BodySplice, MetaSplice, SourceSlice, SpliceTarget, build_meta_nodes};
use crate::providers::syntax::{SyntaxProvider, newline};
use crate::syntax::fragment::FragmentKind;

impl SyntaxProvider {
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
        let resolver = self.resolver_for(source_file)?;

        // The fragment's full definition (decorators + docstring + signature + body).
        // Byte range is resolved lazily at read time by SourceSlice.
        let body_name = format!("{FILE_BODY}.{}", dctx.ext);
        let body_node = VirtualNode::file(&body_name, SourceSlice {
            resolver: resolver.clone(),
            target: SpliceTarget::FragmentBody(fragment_path.to_vec()),
        })
        .with_writable(BodySplice {
            meta: MetaSplice {
                resolver: resolver.clone(),
                target: SpliceTarget::FragmentBody(fragment_path.to_vec()),
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

        // LSP-backed virtual files and symlink directories.
        let lsp_handle = LspHandle::for_file(&self.ctx, source_file);
        if let Some(lsp_handle) = &lsp_handle {
            nodes.extend(build_lsp_symbol_nodes(
                lsp_handle,
                &dctx.shared.source,
                parent_frag.name_byte_offset,
                &self.lsp,
                &resolver,
                fragment_path,
            ));

            // Code actions directory — only if the server supports it.
            if lsp_handle.capabilities().code_action_provider.is_some() {
                nodes.push(VirtualNode::directory(SUBDIR_ACTIONS));
            }
        }

        // Child section fragments (excluding code blocks — those go in code/).
        let section_children: Vec<_> = parent_frag
            .children
            .iter()
            .filter(|c| !matches!(c.kind, FragmentKind::CodeBlock { .. }))
            .cloned()
            .collect();
        nodes.extend(build_fragment_nodes(
            &section_children,
            source_file,
            fragment_path,
            &self.ctx,
            lsp_handle.as_ref().map(|h| (h, dctx.shared.source.as_str())),
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

        let resolver = self.resolver_for(source_file)?;

        let nodes: Vec<VirtualNode> = parent_frag
            .children
            .iter()
            .filter(|c| matches!(c.kind, FragmentKind::CodeBlock { .. }))
            .filter_map(|cb| {
                let fs_name = cb.fs_name.as_deref()?;
                let ext = code_block_extension(&cb.kind);
                let filename = format!("{fs_name}.{ext}");

                let node = VirtualNode::file(&filename, SourceSlice {
                    resolver: resolver.clone(),
                    target: SpliceTarget::CodeBlockBody {
                        parent_path: fragment_path.to_vec(),
                        fs_name: fs_name.to_owned(),
                    },
                })
                .with_writable(BodySplice {
                    meta: MetaSplice {
                        resolver: resolver.clone(),
                        target: SpliceTarget::CodeBlockBody {
                            parent_path: fragment_path.to_vec(),
                            fs_name: fs_name.to_owned(),
                        },
                    },
                });
                Some(newline::with_newline_middlewares(node))
            })
            .collect();

        Ok(Some(nodes))
    }
}
