//! Symbol-scoped git features — per-symbol blame, log, and history.
//!
//! This provider extends nyne-git's file-level git features with
//! syntax-aware symbol scoping. It handles the `symbols/{..path}@/git/`
//! companion routes using `ConflictResolution::Force`.

use std::str::from_utf8;
use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::dispatch::activation::ActivationContext;
use nyne::dispatch::context::RequestContext;
use nyne::dispatch::routing::ctx::RouteCtx;
use nyne::dispatch::routing::tree::RouteTree;
use nyne::node::{Readable, VirtualNode};
use nyne::provider::{Node, Nodes, Provider, ProviderId};
use nyne::templates::{TemplateEngine, TemplateHandle, TemplateView};
use nyne::types::path_conventions::split_companion_path;
use nyne::types::slice::{SliceSpec, parse_spec};
use nyne::types::vfs_path::VfsPath;
use nyne::{companion_children, companion_lookup, source_file};
use nyne_git::names::{self, FILE_BLAME, FILE_LOG};
use nyne_git::{
    BLAME_TEMPLATE, CommitMtime, FileViewCtx, GitRepo, HISTORY_LIMIT, LOG_TEMPLATE, history_filename,
    hunk_overlaps_range,
};
use nyne_macros::routes;

use crate::providers::fragment_resolver::FragmentResolver;
use crate::services::SourceServices;
use crate::syntax::SyntaxRegistry;

/// Provider for symbol-scoped git features.
///
/// Extends nyne-git's file-level blame, log, and history with syntax-aware
/// symbol scoping. For any decomposed symbol, this provider contributes a
/// `git/` companion directory containing blame hunks, commit log entries,
/// and historical versions filtered to that symbol's line range.
///
/// Uses [`ConflictResolution::Force`](nyne::provider::ConflictResolution) to
/// override nyne-git's file-level `git/` directory when accessed through a
/// symbol path (`symbols/{..path}@/git/`).
pub struct GitSymbolsProvider {
    ctx: Arc<ActivationContext>,
    blame_handle: TemplateHandle,
    log_handle: TemplateHandle,
    companion_routes: RouteTree<Self>,
}

/// Methods for [`GitSymbolsProvider`].
impl GitSymbolsProvider {
    /// Unique provider identifier for symbol-scoped git features.
    pub(crate) const PROVIDER_ID: ProviderId = ProviderId::new("git-symbols");

    /// Create a new provider with blame, log, and history route trees.
    pub(crate) fn new(ctx: Arc<ActivationContext>) -> Self {
        let mut b = names::handle_builder();
        let blame_key = b.register("git-symbols/blame", BLAME_TEMPLATE);
        let log_key = b.register("git-symbols/log", LOG_TEMPLATE);
        let engine = b.finish();

        let companion_routes = routes!(Self, {
            "symbols" {
                "{..path}@" {
                    "git" => children_symbol_git {
                        lookup "BLAME.md:{spec}" => lookup_sliced_blame,
                        lookup "LOG.md:{spec}" => lookup_sliced_log,
                        "history" => children_symbol_history,
                    }
                }
            }
        });

        Self {
            ctx,
            blame_handle: TemplateHandle::new(&engine, blame_key),
            log_handle: TemplateHandle::new(&engine, log_key),
            companion_routes,
        }
    }

    /// Retrieve the git repository from the activation context.
    fn repo(&self) -> Result<Arc<GitRepo>> {
        self.ctx
            .get::<Arc<GitRepo>>()
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("git repo not available"))
    }

    /// Build a fragment resolver for the given source file.
    fn fragment_resolver(&self, source: VfsPath) -> FragmentResolver {
        let cache = SourceServices::get(&self.ctx).decomposition.clone();
        FragmentResolver::new(cache, source)
    }

    /// Emit blame, log, and history nodes for a symbol's git directory.
    fn children_symbol_git(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let source = source_file(ctx)?;
        let fragment_path = ctx.params("path").to_vec();
        let resolver = self.fragment_resolver(source.clone());
        let repo = self.repo()?;
        let rel = repo.rel_path(&source);
        let secs = repo.file_epoch_secs(&rel);
        let fctx = FileViewCtx::new(&repo, rel);
        Ok(Some(vec![
            self.blame_handle
                .node(names::FILE_BLAME, SymbolBlameView {
                    ctx: fctx.clone(),
                    resolver: resolver.clone(),
                    fragment_path: fragment_path.clone(),
                    spec: None,
                })
                .with_lifecycle(CommitMtime(secs)),
            self.log_handle
                .node(names::FILE_LOG, SymbolLogView {
                    ctx: fctx,
                    resolver,
                    fragment_path,
                    spec: None,
                })
                .with_lifecycle(CommitMtime(secs)),
            VirtualNode::directory(names::DIR_HISTORY).with_lifecycle(CommitMtime(secs)),
        ]))
    }

    /// Lookup a symbol-scoped blame with an optional line-range spec.
    fn lookup_sliced_blame(&self, ctx: &RouteCtx<'_>) -> Node {
        let Some(spec) = parse_spec(ctx.param("spec")) else {
            return Ok(None);
        };
        let source = source_file(ctx)?;
        let fragment_path = ctx.params("path").to_vec();
        let resolver = self.fragment_resolver(source.clone());
        let repo = self.repo()?;
        let fctx = FileViewCtx::new(&repo, repo.rel_path(&source));
        let spec_label = ctx.param("spec");
        Ok(Some(self.blame_handle.node(
            format!("{FILE_BLAME}:{spec_label}"),
            SymbolBlameView {
                ctx: fctx,
                resolver,
                fragment_path,
                spec: Some(spec),
            },
        )))
    }

    /// Lookup a symbol-scoped log with an optional line-range spec.
    fn lookup_sliced_log(&self, ctx: &RouteCtx<'_>) -> Node {
        let Some(spec) = parse_spec(ctx.param("spec")) else {
            return Ok(None);
        };
        let source = source_file(ctx)?;
        let fragment_path = ctx.params("path").to_vec();
        let resolver = self.fragment_resolver(source.clone());
        let repo = self.repo()?;
        let fctx = FileViewCtx::new(&repo, repo.rel_path(&source));
        let spec_label = ctx.param("spec");
        Ok(Some(self.log_handle.node(
            format!("{FILE_LOG}:{spec_label}"),
            SymbolLogView {
                ctx: fctx,
                resolver,
                fragment_path,
                spec: Some(spec),
            },
        )))
    }

    /// List historical versions of a symbol from git history.
    fn children_symbol_history(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let source = source_file(ctx)?;
        let fragment_path = ctx.params("path");
        let Some(line_range) = self.fragment_resolver(source.clone()).line_range(fragment_path)? else {
            return Ok(None);
        };
        let repo = self.repo()?;
        let rel = repo.rel_path(&source);
        let ext = source.extension().unwrap_or("");
        let shared = Arc::new(SymbolHistoryCtx {
            rel_path: rel.clone(),
            ext: ext.to_owned(),
            fragment_path: fragment_path.to_vec(),
            syntax: self.ctx.get::<Arc<SyntaxRegistry>>().cloned(),
            repo: Arc::clone(&repo),
        });
        let nodes = repo
            .file_history_in_range(&rel, &line_range, HISTORY_LIMIT)?
            .into_iter()
            .enumerate()
            .map(|(i, entry)| {
                let secs = entry.commit.epoch_secs;
                VirtualNode::file(history_filename(i, &entry, ext), SymbolHistoryVersionContent {
                    ctx: Arc::clone(&shared),
                    oid: entry.oid,
                })
                .with_lifecycle(CommitMtime(secs))
            })
            .collect();
        Ok(Some(nodes))
    }
}

/// [`Provider`] implementation for [`GitSymbolsProvider`].
impl Provider for GitSymbolsProvider {
    /// Return the git-symbols provider identifier.
    fn id(&self) -> ProviderId { Self::PROVIDER_ID }

    /// List companion children for symbol git routes.
    fn children(self: Arc<Self>, ctx: &RequestContext<'_>) -> Nodes {
        let Some(split) = split_companion_path(ctx.path) else {
            return Ok(None);
        };
        companion_children(&self.companion_routes, &self, ctx, &split)
    }

    /// Lookup a companion node by name within symbol git routes.
    fn lookup(self: Arc<Self>, ctx: &RequestContext<'_>, name: &str) -> Node {
        let Some(split) = split_companion_path(ctx.path) else {
            return Ok(None);
        };
        companion_lookup(&self.companion_routes, &self, ctx, &split, name)
    }
}

/// Per-symbol blame: filters blame hunks to a symbol's line range,
/// optionally sliced by a [`SliceSpec`] (e.g., `BLAME.md:5` → 5th hunk).
struct SymbolBlameView {
    ctx: FileViewCtx,
    resolver: FragmentResolver,
    fragment_path: Vec<String>,
    spec: Option<SliceSpec>,
}

/// [`TemplateView`] implementation for [`SymbolBlameView`].
impl TemplateView for SymbolBlameView {
    /// Render the symbol-scoped blame via template.
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let Some(line_range) = self.resolver.line_range(&self.fragment_path)? else {
            return Ok(engine.render_bytes(template, &minijinja::context!(data => Vec::<()>::new())));
        };
        let all: Vec<_> = self
            .ctx
            .repo
            .blame(&self.ctx.rel_path)?
            .into_iter()
            .filter(|h| hunk_overlaps_range(h, &line_range))
            .collect();
        let data = self.spec.as_ref().map_or(all.as_slice(), |s| s.apply(&all));
        Ok(engine.render_bytes(template, &minijinja::context!(data)))
    }
}

/// Per-symbol log: filters commits to those touching a symbol's line range,
/// optionally sliced by a [`SliceSpec`] (e.g., `LOG.md:-5` → last 5 commits).
struct SymbolLogView {
    ctx: FileViewCtx,
    resolver: FragmentResolver,
    fragment_path: Vec<String>,
    spec: Option<SliceSpec>,
}

/// [`TemplateView`] implementation for [`SymbolLogView`].
impl TemplateView for SymbolLogView {
    /// Render the symbol-scoped log via template.
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let Some(line_range) = self.resolver.line_range(&self.fragment_path)? else {
            return Ok(engine.render_bytes(template, &minijinja::context!(data => Vec::<()>::new())));
        };
        let all = self
            .ctx
            .repo
            .file_history_in_range(&self.ctx.rel_path, &line_range, HISTORY_LIMIT)?;
        let data = self.spec.as_ref().map_or(all.as_slice(), |s| s.apply(&all));
        Ok(engine.render_bytes(template, &minijinja::context!(data)))
    }
}

/// Shared context for symbol history version lookups.
///
/// Captured once when listing history entries and then shared (via `Arc`)
/// across all [`SymbolHistoryVersionContent`] nodes for that symbol.
/// Avoids redundant repository and registry lookups per version.
struct SymbolHistoryCtx {
    repo: Arc<GitRepo>,
    rel_path: String,
    ext: String,
    fragment_path: Vec<String>,
    syntax: Option<Arc<SyntaxRegistry>>,
}

/// Per-symbol history version: extracts the symbol body from a historical
/// file revision using tree-sitter decomposition. Falls back to full file
/// content when no decomposer exists or the symbol isn't found.
struct SymbolHistoryVersionContent {
    ctx: Arc<SymbolHistoryCtx>,
    oid: git2::Oid,
}

/// [`Readable`] implementation for [`SymbolHistoryVersionContent`].
impl Readable for SymbolHistoryVersionContent {
    /// Read a historical version of the symbol body from a git blob.
    fn read(&self, _ctx: &RequestContext<'_>) -> Result<Vec<u8>> {
        let blob = self.ctx.repo.blob_at(&self.ctx.rel_path, self.oid)?;
        let Ok(source) = from_utf8(&blob) else {
            return Ok(blob);
        };
        let Some(syntax) = &self.ctx.syntax else {
            return Ok(blob);
        };
        match syntax.extract_symbol(source, &self.ctx.ext, &self.ctx.fragment_path) {
            Some(body) => Ok(body.into_bytes()),
            None => Ok(blob),
        }
    }
}
