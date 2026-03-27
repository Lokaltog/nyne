//! Git provider — exposes blame, history, log, contributors, diff, branches, and tags.
//!
//! Creates virtual files/directories under two scopes:
//! - `file.rs@/git/` — per-file git metadata (blame, log, contributors, notes)
//! - `@/git/` — repository-wide browsing (branches, tags, status)
//!
//! Symbol-scoped git features (per-symbol blame/history) live in `nyne-coding`.

use history::HistoryQueries as _;
use nyne::dispatch::routing::ctx::RouteCtx;
use nyne::dispatch::routing::tree::RouteTree;
use nyne::node::{Lifecycle, NodeAttr};
use nyne::prelude::*;
use nyne::templates::TemplateHandle;
use nyne::types::GitDirName;
use nyne::types::slice::SliceSpec;
use nyne::{dispatch_children, dispatch_lookup, source_file};
use nyne_macros::routes;

use crate::names::{
    self as names, DIR_BRANCHES, DIR_DIFF, DIR_GIT, DIR_HISTORY, DIR_TAGS, FILE_BLAME, FILE_GIT_STATUS, FILE_HEAD_DIFF,
    FILE_LOG,
};
use crate::repo::Repo;

/// Define a per-file git view struct backed by `FileViewCtx`.
///
/// Generates a tuple struct wrapping [`FileViewCtx`] and a [`TemplateView`]
/// impl that fetches data from the repo at read time. The `$fetch` expression
/// receives the repo and relative path, and must return `Result<T>` where `T`
/// is serializable.
macro_rules! git_template_view {
    ($(#[$attr:meta])* $name:ident, |$repo:ident, $path:ident| $fetch:expr) => {
        $(#[$attr])*
        pub(super) struct $name(pub super::repo::FileViewCtx);

        impl nyne::templates::TemplateView for $name {
            fn render(
                &self,
                engine: &nyne::templates::TemplateEngine,
                template: &str,
            ) -> color_eyre::eyre::Result<Vec<u8>> {
                let $repo = &self.0.repo;
                let $path = &self.0.rel_path;
                let data = $fetch?;
                Ok(engine.render_bytes(template, &minijinja::context!(data)))
            }
        }
    };
}

/// Blame rendering.
mod blame;
/// Branch browsing and mutation.
mod branches;
/// Contributor ranking.
mod contributors;
/// Diff generation.
mod diff;
/// File history versions.
pub mod history;
/// Commit log rendering.
mod log;
/// Git notes read/write.
mod notes;
/// Repository file view context.
pub mod repo;
/// Working tree status rendering.
mod status;
/// Blame and log template views.
pub mod views;

use branches::{branch_segments_at_prefix, branch_tree_nodes};
use diff::{DiffContent, DiffTarget};
use status::StatusView;
use views::{SlicedBlameView, SlicedLogView};

/// Lifecycle that reports a git commit timestamp as the node's mtime.
///
/// Attached to history version nodes so that `ls -l` shows the commit date
/// rather than the current time. The inner value is seconds since epoch.
pub struct CommitMtime(pub i64);

/// [`Lifecycle`] implementation for [`CommitMtime`].
impl Lifecycle for CommitMtime {
    /// Returns node attributes with mtime from the commit timestamp.
    fn getattr(&self, _ctx: &RequestContext<'_>) -> Option<NodeAttr> {
        Some(NodeAttr {
            mtime: Some(u64::try_from(self.0).unwrap_or(0)),
            ..NodeAttr::default()
        })
    }
}
/// Extension trait to attach [`CommitMtime`] lifecycle to a node.
pub trait CommitMtimeExt {
    /// Attach a [`CommitMtime`] lifecycle with the given epoch seconds.
    #[must_use]
    fn with_mtime(self, epoch_secs: i64) -> Self;
}

/// Blanket [`CommitMtimeExt`] implementation for [`VirtualNode`].
impl CommitMtimeExt for VirtualNode {
    /// Attach a [`CommitMtime`] lifecycle so the node reports the commit timestamp as mtime.
    fn with_mtime(self, epoch_secs: i64) -> Self { self.with_lifecycle(CommitMtime(epoch_secs)) }
}

/// Template handles for git-backed virtual files.
pub(crate) struct Handles {
    pub(crate) blame: TemplateHandle,
    pub(crate) log: TemplateHandle,
    pub(crate) contributors: TemplateHandle,
    pub(crate) status: TemplateHandle,
    pub(crate) notes: TemplateHandle,
}

/// Git provider for file-level blame, history, diff, log, contributors, and
/// repository browsing (branches, tags).
///
/// Symbol-scoped git features (per-symbol blame/history) are provided by
/// nyne-coding's git-symbols extension.
pub(crate) struct GitProvider {
    ctx: Arc<ActivationContext>,
    handles: Handles,
    git_dir_component: Option<String>,
    at_routes: RouteTree<Self>,
    companion_routes: RouteTree<Self>,
}

/// Construction, route handlers, and helper methods for [`GitProvider`].
impl GitProvider {
    /// Provider identifier for git.
    pub(crate) const PROVIDER_ID: ProviderId = ProviderId::new("git");

    /// Creates a new git provider with routes and template handles.
    pub(crate) fn new(ctx: Arc<ActivationContext>) -> Self {
        let git_dir_component = ctx.get::<GitDirName>().map(|g| g.0.clone());

        let mut b = names::handle_builder();
        let blame_key = b.register("git/blame", views::BLAME_TEMPLATE);
        let log_key = b.register("git/log", views::LOG_TEMPLATE);
        let contributors_key = b.register("git/contributors", include_str!("templates/contributors.md.j2"));
        let status_key = b.register("git/status", include_str!("templates/status.md.j2"));
        let notes_key = b.register("git/notes", include_str!("templates/notes.md.j2"));
        let engine = b.finish();
        let handles = Handles {
            blame: TemplateHandle::new(&engine, blame_key),
            log: TemplateHandle::new(&engine, log_key),
            contributors: TemplateHandle::new(&engine, contributors_key),
            status: TemplateHandle::new(&engine, status_key),
            notes: TemplateHandle::new(&engine, notes_key),
        };

        let at_routes = routes!(Self, {
            no_emit "@" {
                "git" => children_git_root {
                    "branches" => children_branches {
                        "{..prefix}" => children_branches_nested,
                    }
                    "tags" => children_tags,
                }
            }
        });

        let companion_routes = routes!(Self, {
            children(children_companion_root),
            "git" => children_companion_git {
                lookup "BLAME.md:{spec}" => lookup_sliced_blame,
                lookup "LOG.md:{spec}" => lookup_sliced_log,
            }
            "diff" => children_diff {
                lookup "{ref}.diff" => lookup_diff_ref,
            }
            "history" => children_history,
        });

        Self {
            ctx,
            handles,
            git_dir_component,
            at_routes,
            companion_routes,
        }
    }

    /// Returns the shared git repository.
    fn repo(&self) -> Result<Arc<Repo>> {
        self.ctx
            .get::<Arc<Repo>>()
            .cloned()
            .ok_or_else(|| color_eyre::eyre::eyre!("git repo not available"))
    }

    /// Extracts the source file, git repo, and repo-relative path from a route context.
    fn file_ctx(&self, ctx: &RouteCtx<'_>) -> Result<(VfsPath, Arc<Repo>, String)> {
        let source = source_file(ctx)?;
        let repo = self.repo()?;
        let rel = repo.rel_path(&source);
        Ok((source, repo, rel))
    }

    /// Lists children at the git root (branches, tags, status).
    fn children_git_root(&self, _ctx: &RouteCtx<'_>) -> Nodes {
        let repo = self.repo()?;
        let secs = repo.head_epoch_secs();
        Ok(Some(vec![
            VirtualNode::directory(DIR_BRANCHES).with_mtime(secs),
            VirtualNode::directory(DIR_TAGS).with_mtime(secs),
            self.handles
                .status
                .node(FILE_GIT_STATUS, StatusView { repo })
                .with_cache_policy(CachePolicy::Never)
                .with_mtime(secs),
        ]))
    }

    /// Lists top-level branch name segments.
    fn children_branches(&self, _ctx: &RouteCtx<'_>) -> Nodes { branch_segments_at_prefix(&self.repo()?, "") }

    /// Resolves nested branch segments or browses a branch tree.
    fn children_branches_nested(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let repo = self.repo()?;
        let segs = ctx.params("prefix");

        // Try as a branch namespace prefix first (e.g., segs=["feat"] for feat/foo, feat/bar).
        let mut ns_prefix = segs.join("/");
        ns_prefix.push('/');
        if let Some(nodes) = branch_segments_at_prefix(&repo, &ns_prefix)? {
            return Ok(Some(nodes));
        }

        // Not a namespace — find the longest branch name that is a prefix of the segments.
        // e.g., segs=["main","src"] → branch "main", tree_path "src"
        // e.g., segs=["feat","foo","src"] → branch "feat/foo", tree_path "src"
        let mut branches = repo.branches()?;
        branches.sort();
        #[allow(clippy::indexing_slicing)] // split ∈ 1..=segs.len() — always in bounds
        for split in (1..=segs.len()).rev() {
            let candidate = segs[..split].join("/");
            if branches.binary_search(&candidate).is_ok() {
                return branch_tree_nodes(&repo, &candidate, &segs[split..].join("/"));
            }
        }

        Ok(None)
    }

    /// Lists all tags.
    fn children_tags(&self, _ctx: &RouteCtx<'_>) -> Nodes {
        let repo = self.repo()?;
        let head_mtime = repo.head_epoch_secs();
        let tags = repo.tags()?;
        let nodes = tags
            .iter()
            .map(|name| VirtualNode::directory(name).with_mtime(head_mtime))
            .collect();
        Ok(Some(nodes))
    }

    /// Lists companion directories (git, history, diff) for a source file.
    fn children_companion_root(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let (_source, repo, rel) = self.file_ctx(ctx)?;
        let secs = repo.file_epoch_secs(&rel);
        Ok(Some(vec![
            VirtualNode::directory(DIR_GIT).with_mtime(secs),
            VirtualNode::directory(DIR_HISTORY).with_mtime(secs),
            VirtualNode::directory(DIR_DIFF).with_mtime(secs),
        ]))
    }

    /// Lists diff children for a source file.
    fn children_diff(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let (source, repo, rel) = self.file_ctx(ctx)?;
        let secs = repo.head_epoch_secs();
        Ok(Some(vec![
            VirtualNode::file(FILE_HEAD_DIFF, DiffContent {
                repo: Arc::clone(&repo),
                rel_path: rel,
                target: DiffTarget::Workdir { source_file: source },
            })
            .with_mtime(secs),
        ]))
    }

    /// Lists companion git children (blame, log, etc.) for a source file.
    fn children_companion_git(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let (_source, repo, rel) = self.file_ctx(ctx)?;
        Ok(Some(self.resolve_companion_git(&repo, rel)))
    }

    /// Shared implementation for line-range-sliced view lookups.
    fn lookup_sliced_view<V: TemplateView + 'static>(
        &self,
        ctx: &RouteCtx<'_>,
        handle: &TemplateHandle,
        file_name: &str,
        make_view: impl FnOnce(repo::FileViewCtx, SliceSpec) -> V,
    ) -> Node {
        use nyne::types::slice::parse_spec;
        let Some(spec) = parse_spec(ctx.param("spec")) else {
            return Ok(None);
        };
        let source = source_file(ctx)?;
        let repo = self.repo()?;
        let fctx = repo::FileViewCtx::new(&repo, repo.rel_path(&source));
        let spec_label = ctx.param("spec");
        Ok(Some(
            handle.node(format!("{file_name}:{spec_label}"), make_view(fctx, spec)),
        ))
    }

    /// Looks up a line-range-sliced blame view.
    fn lookup_sliced_blame(&self, ctx: &RouteCtx<'_>) -> Node {
        self.lookup_sliced_view(ctx, &self.handles.blame, FILE_BLAME, |fctx, spec| SlicedBlameView {
            ctx: fctx,
            spec,
        })
    }

    /// Looks up a line-range-sliced log view.
    fn lookup_sliced_log(&self, ctx: &RouteCtx<'_>) -> Node {
        self.lookup_sliced_view(ctx, &self.handles.log, FILE_LOG, |fctx, spec| SlicedLogView {
            ctx: fctx,
            spec,
        })
    }

    /// Looks up a diff against a named ref.
    fn lookup_diff_ref(&self, ctx: &RouteCtx<'_>) -> Node {
        let refspec = ctx.param("ref");
        if refspec == "HEAD" {
            return Ok(None);
        }
        let (source, repo, _rel) = self.file_ctx(ctx)?;
        Ok(Some(VirtualNode::file(format!("{refspec}.diff"), DiffContent {
            repo: Arc::clone(&repo),
            rel_path: repo.rel_path(&source),
            target: DiffTarget::Ref(refspec.to_owned()),
        })))
    }

    /// Lists file history version entries.
    fn children_history(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let (source, repo, rel) = self.file_ctx(ctx)?;
        let ext = source.extension().unwrap_or("");
        let entries = repo.file_history(&rel, views::HISTORY_LIMIT)?;
        let rel: Arc<str> = Arc::from(rel);
        let nodes = entries
            .into_iter()
            .enumerate()
            .map(|(i, entry)| {
                let filename = views::history_filename(i, &entry, ext);
                let secs = entry.epoch_secs;
                VirtualNode::file(filename, history::HistoryVersionContent {
                    repo: Arc::clone(&repo),
                    rel_path: Arc::clone(&rel),
                    oid: entry.oid,
                })
                .with_mtime(secs)
            })
            .collect();
        Ok(Some(nodes))
    }
}

/// [`Provider`] implementation for [`GitProvider`].
impl Provider for GitProvider {
    /// Returns the provider identifier.
    fn id(&self) -> ProviderId { Self::PROVIDER_ID }

    /// Dispatches child listing to at-root or companion routes.
    fn children(self: Arc<Self>, ctx: &RequestContext<'_>) -> Nodes {
        dispatch_children(&self.at_routes, &self.companion_routes, &self, ctx, true)
    }

    /// Dispatches name lookup to at-root or companion routes.
    fn lookup(self: Arc<Self>, ctx: &RequestContext<'_>, name: &str) -> Node {
        dispatch_lookup(&self.at_routes, &self.companion_routes, &self, ctx, name, true)
    }

    /// Invalidates provider state when git directory changes.
    fn on_fs_change(&self, changed: &[VfsPath]) -> Vec<InvalidationEvent> {
        let Some(git_dir) = &self.git_dir_component else {
            return Vec::new();
        };
        let dominated_by_git = changed
            .iter()
            .any(|p| p.components().next().is_some_and(|first| first == git_dir.as_str()));
        if !dominated_by_git {
            return Vec::new();
        }
        vec![InvalidationEvent::Provider { provider_id: self.id() }]
    }
}

/// Unit tests.
#[cfg(test)]
mod tests;
