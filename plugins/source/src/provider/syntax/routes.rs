//! Route tree and handler functions for the syntax provider.

use std::path::Path;
use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::router::{NamedNode, Node, NodeKind, Request, RouteCtx, RouteTree, slice_node};
use nyne_companion::{Companion, CompanionRequest};
use nyne_diff::DiffUnlinkable;

use super::SyntaxProvider;
use super::content::{FileOverviewContent, LinesContent, LinesWrite, Slice, SpliceTarget, delete, file_docstring_node};
use crate::edit::plan::EditOpKind;
use crate::edit::staging::StageWritable;
use crate::extensions::SourceExtensions;
use crate::plugin::config::vfs::Vfs;
use crate::syntax::find_fragment;
use crate::syntax::fragment::{FragmentKind, find_fragment_of_kind};

#[allow(clippy::excessive_nesting, clippy::too_many_lines)]
/// Build the route tree for the syntax provider.
///
/// ```text
/// file.rs@/
/// ├── OVERVIEW.md              (content: file overview template)
/// ├── lines                    (content: full-file lines, readable + writable)
/// └── symbols/
///     ├── OVERVIEW.md          (content: symbols overview template)
///     ├── imports.<ext>        (content: import block slice)
///     ├── docstring.txt        (content: file-level docstring slice)
///     ├── by-kind/             (readdir: list distinct symbol kinds)
///     │   └── {kind}/          (readdir: list symbols of a specific kind)
///     ├── at-line/             (lookup: line number → symlink)
///     └── {..path}             (on_readdir + on_lookup: fragment callbacks)
/// ```
///
/// The tree is only dispatched for parseable source files — `accept()` gates
/// on `decomposer_for()` before entering the tree.
pub fn build_tree(vfs: &Vfs, ext: &SourceExtensions) -> RouteTree<SyntaxProvider> {
    RouteTree::builder()
        // file.rs@/OVERVIEW.md — file-level overview with metadata and symbol listing
        .content(|p: &SyntaxProvider, _ctx, req| {
            let sf = req.source_file()?;
            Some(p.file_overview.named_node(&p.vfs.file.overview, FileOverviewContent {
                language: p.decomposer_for(&sf)?.language_name().to_owned(),
                resolver: p.resolver_for(&sf),
                filename: sf.file_name()?.to_str()?.to_owned(),
            }))
        })
        // file.rs@/lines — full source with line-range read and splice write
        .content(|p: &SyntaxProvider, _ctx, req| {
            let sf = req.source_file()?;
            let decomposer = p.decomposer_for(&sf)?;
            Some(
                Node::file()
                    .with_readable(LinesContent {
                        source_file: sf.clone(),
                    })
                    .with_writable(LinesWrite {
                        source_file: sf.clone(),
                        decomposer: Arc::clone(decomposer),
                        resolver: p.resolver_for(&sf),
                    })
                    .named("lines"),
            )
        })
        .dir(vfs.dir.symbols.clone(), |d| {
            d
                // symbols/ readdir — emit top-level fragment directories
                .on_readdir(|p: &SyntaxProvider, _ctx, req| {
                    let Some((companion, sf)) = req.companion_context() else {
                        return Ok(());
                    };
                    if let Some(nodes) = p.resolve_symbols_root(&companion, &sf)? {
                        req.nodes.extend(nodes);
                    }
                    Ok(())
                })
                // symbols/ lookup — shorthand: symbols/Foo.rs → symlink
                .on_lookup(|p: &SyntaxProvider, _ctx, req, name| {
                    let Some((companion, sf)) = req.companion_context() else {
                        return Ok(());
                    };
                    if let Some(node) = p.lookup_symbol_shorthand(&companion, &sf, name)? {
                        req.nodes.add(node);
                    }
                    Ok(())
                })
                // symbols/OVERVIEW.md — symbol listing with kinds and line ranges
                .content(|p: &SyntaxProvider, _ctx, req| {
                    let sf = req.source_file()?;
                    Some(p.overview.named_node(
                        &p.vfs.file.overview,
                        super::content::OverviewContent {
                            resolver: p.resolver_for(&sf),
                            filename: sf.file_name()?.to_str()?.to_owned(),
                        },
                    ))
                })
                // symbols/imports.<ext> — import block (conditional on imports existing)
                .content(|p: &SyntaxProvider, _ctx, req| {
                    let sf = req.source_file()?;
                    let dctx = p.decomposition_context(&sf).ok()??;
                    find_fragment_of_kind(&dctx.shared.decomposed, &FragmentKind::Imports)?;
                    let resolver = p.resolver_for(&sf);
                    Some(slice_node(
                        format!("{}.{}", p.vfs.file.imports, dctx.ext),
                        Slice {
                            resolver: resolver.clone(),
                            target: SpliceTarget::Imports,
                        },
                        super::content::MetaSplice {
                            resolver,
                            target: SpliceTarget::Imports,
                        },
                    ))
                })
                // symbols/docstring.txt — file-level docstring (conditional on existing)
                .content(|p: &SyntaxProvider, _ctx, req| {
                    let sf = req.source_file()?;
                    let dctx = p.decomposition_context(&sf).ok()??;
                    find_fragment_of_kind(&dctx.shared.decomposed, &FragmentKind::Docstring)?;
                    let resolver = p.resolver_for(&sf);
                    Some(file_docstring_node(&resolver, &p.vfs.file))
                })
                .dir(vfs.dir.by_kind.clone(), |d| {
                    d
                        // by-kind/ readdir — list distinct symbol kinds as directories
                        .on_readdir(|p: &SyntaxProvider, _ctx, req| {
                            let Some(sf) = req.source_file() else {
                                return Ok(());
                            };
                            if let Some(nodes) = p.resolve_by_kind_root(&sf)? {
                                req.nodes.extend(nodes);
                            }
                            Ok(())
                        })
                        .capture("kind", |d| {
                            // by-kind/{kind}/ readdir — list symbols matching the captured kind
                            d.on_readdir(|p: &SyntaxProvider, ctx, req| {
                                let Some((companion, sf)) = req.companion_context() else {
                                    return Ok(());
                                };
                                let Some(kind) = ctx.param("kind") else {
                                    return Ok(());
                                };
                                if let Some(nodes) = p.resolve_by_kind_filter(&companion, &sf, kind)? {
                                    req.nodes.extend(nodes);
                                }
                                Ok(())
                            })
                        })
                })
                .dir(vfs.dir.at_line.clone(), |d| {
                    // at-line/ lookup — resolve line number to symbol symlink
                    d.on_lookup(|p: &SyntaxProvider, _ctx, req, name| {
                        let Some((companion, sf)) = req.companion_context() else {
                            return Ok(());
                        };
                        if let Some(node) = p.lookup_at_line_impl(&companion, &sf, name)? {
                            req.nodes.add(node);
                        }
                        Ok(())
                    })
                })
                .rest("path", |d| {
                    // {..path} — fragment directory traversal + downstream extensions
                    d.on_readdir(SyntaxProvider::fragment_readdir)
                        .on_lookup(SyntaxProvider::fragment_lookup)
                        .apply(&ext.fragment_path)
                })
        })
        .build()
}

/// Classification of a `symbols/{..path}` path for routing.
///
/// Produced by [`SyntaxProvider::classify_fragment_path`] to keep the
/// `code/` / `edit/` sub-route detection out of both `fragment_readdir`
/// and `fragment_lookup`.
enum FragmentSubRoute<'a> {
    /// `Foo@/bar@/code` — code block directory under a document section.
    CodeBlocks { parent: &'a [String] },
    /// `Foo@/edit` — batch edit staging directory for a fragment.
    Edit { parent: &'a [String] },
    /// The path addresses a fragment directory itself.
    Fragment,
}

impl SyntaxProvider {
    #[allow(clippy::unnecessary_wraps)]
    /// Classify a `symbols/{..path}` route into one of the sub-route variants.
    fn classify_fragment_path<'a>(&self, segments: &'a [String]) -> FragmentSubRoute<'a> {
        match segments.split_last() {
            Some((last, parent)) if last == "code" && !parent.is_empty() => FragmentSubRoute::CodeBlocks { parent },
            Some((last, parent)) if *last == self.vfs.dir.edit && !parent.is_empty() =>
                FragmentSubRoute::Edit { parent },
            _ => FragmentSubRoute::Fragment,
        }
    }

    /// Readdir callback for `symbols/{..path}` — fragment directories (Foo@/, Foo@/Bar@/).
    ///
    /// Lists fragment children (body, meta-files, child symbols, code/) or
    /// dispatches to `code/` / `edit/` sub-routes when the path ends with one.
    pub(super) fn fragment_readdir(&self, ctx: &RouteCtx, req: &mut Request) -> Result<()> {
        let Some((companion, sf)) = req.companion_context() else {
            return Ok(());
        };
        let Some(path_param) = ctx.param("path") else {
            return Ok(());
        };
        let segments: Vec<String> = path_param.split('/').map(String::from).collect();

        match self.classify_fragment_path(&segments) {
            FragmentSubRoute::CodeBlocks { parent } =>
                if let Some(nodes) = self.resolve_code_block_dir(&sf, parent)? {
                    req.nodes.extend(nodes);
                },
            FragmentSubRoute::Edit { parent } =>
                if self.decomposition.has_fragment(&sf, parent) {
                    for kind in <EditOpKind as strum::IntoEnumIterator>::iter() {
                        req.nodes.add(NamedNode::file(kind.name()));
                    }
                },
            FragmentSubRoute::Fragment =>
                if let Some(nodes) = self.resolve_fragment_dir(&companion, &sf, &segments)? {
                    req.nodes.extend(nodes);
                },
        }
        Ok(())
    }

    #[allow(clippy::unnecessary_wraps)]
    /// Lookup callback for `symbols/{..path}` — resolves individual files
    /// within fragment directories.
    pub(super) fn fragment_lookup(&self, ctx: &RouteCtx, req: &mut Request, name: &str) -> Result<()> {
        let Some((companion, sf)) = req.companion_context() else {
            return Ok(());
        };
        let Some(path_param) = ctx.param("path") else {
            return Ok(());
        };
        let segments: Vec<String> = path_param.split('/').map(String::from).collect();

        match self.classify_fragment_path(&segments) {
            FragmentSubRoute::CodeBlocks { parent } => {
                if let Some(nodes) = self.resolve_code_block_dir(&sf, parent)?
                    && let Some(node) = nodes.into_iter().find(|n| n.name() == name)
                {
                    req.nodes.add(node);
                }
            }
            FragmentSubRoute::Edit { parent } => {
                if let Ok(kind) = name.parse::<EditOpKind>()
                    && self.decomposition.has_fragment(&sf, parent)
                {
                    req.nodes.add(
                        Node::file()
                            .with_writable(StageWritable {
                                staging: self.staging.clone(),
                                source_file: sf,
                                fragment_path: parent.to_vec(),
                                kind,
                            })
                            .named(name),
                    );
                }
            }
            FragmentSubRoute::Fragment => {
                // delete.diff — handled by the diff middleware.
                if name == self.vfs.file.delete_diff {
                    self.set_delete_diff_source(req, &sf, &segments);
                    return Ok(());
                }
                if let Some(nodes) = self.resolve_fragment_dir(&companion, &sf, &segments)?
                    && let Some(node) = nodes.into_iter().find(|n| n.name() == name)
                {
                    req.nodes.add(node);
                }
                self.attach_unlinkables(req, &companion, &sf, &segments);
            }
        }
        Ok(())
    }

    /// Attach `Unlinkable` to fragment directory nodes that don't already have one.
    ///
    /// Mirrors the LSP plugin's `attach_renameables` pattern. Iterates all
    /// directory nodes in `req.nodes`, strips the companion suffix, resolves
    /// the fragment, and attaches a [`DiffUnlinkable`] for rmdir.
    ///
    /// `context_segments` is the captured `{..path}` from the route tree.
    /// When a node's bare name matches the last segment (self-lookup via
    /// `dispatch_into_rest`), the fragment path is `context_segments` itself.
    /// Otherwise the node is a child, so the fragment path is
    /// `context_segments + [bare_name]`.
    fn attach_unlinkables(
        &self,
        req: &mut Request,
        companion: &Companion,
        source_file: &Path,
        context_segments: &[String],
    ) {
        let Ok(shared) = self.decomposition.get(source_file) else {
            return;
        };

        for node in req.nodes.iter_mut() {
            if node.kind() != NodeKind::Directory || node.unlinkable().is_some() {
                continue;
            }
            let node_name = node.name().to_owned();
            let Some(bare_name) = companion.strip_suffix(&node_name) else {
                continue;
            };

            // If the bare name matches the last context segment, this is a
            // self-lookup (dispatch_into_rest captured the name as the path).
            // The fragment path is context_segments itself.
            // Otherwise, the node is a child — append bare_name.
            let frag_path = if context_segments.last().is_some_and(|last| last == bare_name) {
                context_segments.to_vec()
            } else {
                let mut p = context_segments.to_vec();
                p.push(bare_name.to_owned());
                p
            };

            if find_fragment(&shared.decomposed, &frag_path).is_none() {
                continue;
            }
            node.set_unlinkable(self.delete_unlinkable(source_file, frag_path));
        }
    }

    /// Build a `DiffUnlinkable` for a fragment's delete capability.
    ///
    /// SSOT for the delete capability used by both `build_fragment_nodes`
    /// (readdir) and `attach_unlinkables` (lookup).
    pub(super) fn delete_unlinkable(&self, source_file: &Path, fragment_path: Vec<String>) -> DiffUnlinkable {
        DiffUnlinkable::new(
            delete::SymbolDelete {
                decomposition: self.decomposition.clone(),
                source_file: source_file.to_path_buf(),
                fragment_path,
            },
            Arc::clone(&self.fs),
        )
    }
}
