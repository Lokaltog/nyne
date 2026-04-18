//! Fragment directory resolution — builds the per-symbol `Foo@/` directory
//! with body, meta-files, code blocks, and child symbol entries.

use std::path::Path;

use color_eyre::eyre::Result;
use nyne::router::{NamedNode, Node, Permissions, slice_node};
use nyne_companion::Companion;

use super::{build_fragment_nodes, code_block_extension};
use crate::provider::syntax::SyntaxProvider;
use crate::provider::syntax::content::{BodySplice, FragmentPath, MetaSplice, Slice, SpliceTarget, build_meta_nodes};
use crate::syntax::fragment::FragmentKind;

/// Fragment directory resolution methods for [`SyntaxProvider`].
impl SyntaxProvider {
    /// Resolve a fragment directory (`Foo@/`), assembling its full child listing.
    ///
    /// Emits: `body.<ext>`, meta-files (signature, docstring, decorators, OVERVIEW),
    /// child fragments, and a `code/` directory for fenced code blocks.
    pub(in super::super) fn resolve_fragment_dir(
        &self,
        companion: &Companion,
        source_file: &Path,
        fragment_path: &[String],
    ) -> Result<Option<Vec<NamedNode>>> {
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
        // Byte range is resolved lazily at read time by Slice.
        let frag_path = FragmentPath::new(fragment_path);
        nodes.push(slice_node(
            format!("{}.{}", self.vfs.file.body, dctx.ext),
            Slice {
                resolver: resolver.clone(),
                target: SpliceTarget::FragmentBody(frag_path.clone()),
            },
            BodySplice {
                meta: MetaSplice {
                    resolver: resolver.clone(),
                    target: SpliceTarget::FragmentBody(frag_path),
                },
            },
        ));

        // Per-symbol meta-files (signature, docstring, decorators, overview).
        nodes.extend(build_meta_nodes(
            parent_frag,
            dctx.ext,
            &self.overview,
            &resolver,
            fragment_path,
            &self.vfs.file,
        ));

        // Child section fragments (excluding code blocks — those go in code/).
        nodes.extend(build_fragment_nodes(
            self,
            companion,
            &parent_frag
                .children
                .iter()
                .filter(|c| !matches!(c.kind, FragmentKind::CodeBlock { .. }))
                .collect::<Vec<_>>(),
            source_file,
            fragment_path,
        ));

        // code/ directory if this fragment has code block children.
        if parent_frag
            .children
            .iter()
            .any(|c| matches!(c.kind, FragmentKind::CodeBlock { .. }))
        {
            nodes.push(NamedNode::dir(&self.vfs.dir.code));
        }

        // edit/ directory for batch edit staging operations.
        //
        // Marked writable so the FUSE bridge's `is_writable_dir` pre-check
        // lets `create(2)` through — the staging endpoints (`insert-before`,
        // `insert-after`, `append`, `delete`, `replace`) are materialized
        // by the `on_create` callback in [`build_tree`](super::super::routes::build_tree)
        // as write-only ephemeral nodes. The directory itself has no
        // `Writable` / `Unlinkable` capability, so permissions must be set
        // explicitly.
        nodes.push(Node::dir().with_permissions(Permissions::ALL).named(&self.vfs.dir.edit));

        Ok(Some(nodes))
    }

    /// Resolve `code/` subdirectory under a document section — list fenced code
    /// blocks as editable files with language-derived extensions.
    pub(in super::super) fn resolve_code_block_dir(
        &self,
        source_file: &Path,
        fragment_path: &[String],
    ) -> Result<Option<Vec<NamedNode>>> {
        let Some(dctx) = self.decomposition_context(source_file)? else {
            return Ok(None);
        };

        let Some(parent_frag) = dctx.find_fragment(fragment_path) else {
            return Ok(None);
        };

        let resolver = self.resolver_for(source_file);
        let parent_path = FragmentPath::new(fragment_path);

        let nodes: Vec<NamedNode> = parent_frag
            .children
            .iter()
            .filter(|c| matches!(c.kind, FragmentKind::CodeBlock { .. }))
            .filter_map(|cb| {
                let fs_name = cb.fs_name.as_deref()?;
                let ext = code_block_extension(&cb.kind);
                let target = || SpliceTarget::CodeBlockBody {
                    parent_path: parent_path.clone(),
                    fs_name: fs_name.to_owned(),
                };

                Some(slice_node(
                    format!("{fs_name}.{ext}"),
                    Slice {
                        resolver: resolver.clone(),
                        target: target(),
                    },
                    BodySplice {
                        meta: MetaSplice {
                            resolver: resolver.clone(),
                            target: target(),
                        },
                    },
                ))
            })
            .collect();

        Ok(Some(nodes))
    }
}
