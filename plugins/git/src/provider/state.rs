use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::router::{NamedNode, Request, WriteContext};
use nyne::templates::{HandleBuilder, TemplateEngine, TemplateGlobals, TemplateHandle};
use nyne::{SliceSpec, parse_spec};
use nyne_companion::CompanionRequest;

use super::views;
use crate::history::{self, HistoryQueries as _};
use crate::plugin::config::Limits;
use crate::plugin::config::vfs::Vfs;
use crate::repo::Repo;

/// Template handles for git-backed virtual files.
pub struct Handles {
    pub(crate) blame: TemplateHandle,
    pub(crate) log: TemplateHandle,
    pub(crate) contributors: TemplateHandle,
    pub(crate) status: TemplateHandle,
    pub(crate) notes: TemplateHandle,
}
/// Shared git state for extension callbacks and the provider.
///
/// Created once during plugin activation, stored in [`ActivationContext`],
/// and shared between companion extension callbacks and [`GitProvider`].
///
/// [`ActivationContext`]: nyne::dispatch::activation::ActivationContext
pub struct GitState {
    pub(crate) repo: Arc<Repo>,
    pub(crate) handles: Handles,
    pub(crate) vfs: Vfs,
    pub(crate) limits: Limits,
}
/// Build template handles from VFS config.
///
/// Registers VFS path names as template globals so templates can reference
/// them (e.g. `{{ FILE_BLAME }}`) without hard-coding string literals.
pub fn build_handles(vfs: &Vfs) -> Handles {
    let mut b = HandleBuilder::new();
    vfs.register_globals(b.engine_mut());

    let blame_key = b.register("git/blame", views::BLAME_TEMPLATE);
    let log_key = b.register("git/log", views::LOG_TEMPLATE);
    let contributors_key = b.register("git/contributors", include_str!("templates/contributors.md.j2"));
    let status_key = b.register("git/status", include_str!("templates/status.md.j2"));
    let notes_key = b.register("git/notes", include_str!("templates/notes.md.j2"));
    let engine = b.finish();

    Handles {
        blame: TemplateHandle::new(&engine, blame_key),
        log: TemplateHandle::new(&engine, log_key),
        contributors: TemplateHandle::new(&engine, contributors_key),
        status: TemplateHandle::new(&engine, status_key),
        notes: TemplateHandle::new(&engine, notes_key),
    }
}
impl GitState {
    /// Create a read-only template node scoped to the request's source file.
    pub(crate) fn file_content(
        &self,
        req: &Request,
        handle: &TemplateHandle,
        name: impl Into<String>,
        fetch: impl Fn(&Repo, &FetchCtx<'_>) -> Result<minijinja::Value> + Send + Sync + 'static,
    ) -> Option<NamedNode> {
        let scope = FetchScope::File { source: req.source_file()? };
        Some(handle.lazy_node(name, build_read_fn(Arc::clone(&self.repo), scope, fetch)))
    }

    /// Create an editable template node scoped to the request's source file.
    pub(crate) fn editable_file_content(
        &self,
        req: &Request,
        handle: &TemplateHandle,
        name: impl Into<String>,
        fetch: impl Fn(&Repo, &FetchCtx<'_>) -> Result<minijinja::Value> + Send + Sync + 'static,
        write_fn: impl Fn(&Repo, &str, &[u8]) -> Result<()> + Send + Sync + 'static,
    ) -> Option<NamedNode> {
        let source = req.source_file()?;
        let read_fn = build_read_fn(Arc::clone(&self.repo), FetchScope::File { source: source.clone() }, fetch);
        let write_repo = Arc::clone(&self.repo);
        Some(handle.editable_lazy_node(name, read_fn, move |_ctx: &WriteContext<'_>, data: &[u8]| {
            let rel = write_repo.rel_path(&source);
            write_fn(&write_repo, &rel, data)?;
            Ok(vec![source.clone()])
        }))
    }

    /// Return the request's source file, or a canonical error if absent.
    ///
    /// Use from fallible callback paths where a missing source file is a
    /// contract violation; use [`Request::source_file`] directly in
    /// silently-skipping paths.
    pub(crate) fn require_source_file(req: &Request) -> Result<std::path::PathBuf> {
        req.source_file().ok_or_else(|| color_eyre::eyre::eyre!("no source file"))
    }

    /// Resolve a sliced view name (`BLAME.md:{spec}` or `LOG.md:{spec}`)
    /// to a template handle, parsed spec, and whether it's a blame view.
    ///
    /// Returns `None` if the name doesn't match the expected format.
    pub(crate) fn resolve_sliced_view<'a>(&'a self, name: &str) -> Option<(&'a TemplateHandle, SliceSpec, bool)> {
        let (file_name, spec_str) = name.split_once(':')?;
        let spec = parse_spec(spec_str)?;
        let is_blame = file_name == self.vfs.file.blame;
        let handle = if is_blame {
            &self.handles.blame
        } else if file_name == self.vfs.file.log {
            &self.handles.log
        } else {
            return None;
        };
        Some((handle, spec, is_blame))
    }

    /// Build the fetch closure for a sliced view (`BLAME.md:{spec}` /
    /// `LOG.md:{spec}`), covering both file-level and symbol-level scopes.
    ///
    /// The closure branches on `ctx.range` to apply the slice appropriately:
    /// `None` slices the full-file dataset; `Some(range)` first clamps to
    /// the symbol's range and then applies the spec.
    pub(crate) fn sliced_fetch(
        &self,
        spec: SliceSpec,
        is_blame: bool,
    ) -> impl Fn(&Repo, &FetchCtx<'_>) -> Result<minijinja::Value> + Send + Sync + 'static + use<> {
        let log_limit = self.limits.log;
        let history_limit = self.limits.history;
        move |repo, ctx| {
            Ok(if is_blame {
                let hunks = repo.blame(ctx.rel)?;
                match ctx.range {
                    None => minijinja::context!(data => history::slice_blame_hunks(hunks, &spec)),
                    Some(r) => minijinja::context!(data => spec.apply(&history::filter_blame_to_range(hunks, r))),
                }
            } else {
                match ctx.range {
                    None => minijinja::context!(data => spec.apply(&repo.file_history(ctx.rel, log_limit)?)),
                    Some(r) => {
                        minijinja::context!(data => spec.apply(&repo.file_history_in_range(ctx.rel, r, history_limit)?))
                    }
                }
            })
        }
    }
}

/// Scope for a lazy git-backed template node.
///
/// File-level scope resolves `rel_path` from `source`. Symbol-level scope
/// additionally resolves the symbol's line range at render time via
/// [`FragmentResolver`], returning empty data if the symbol no longer
/// exists in the source.
pub(crate) enum FetchScope {
    /// File-level: `ctx.range` is always `None` in the fetch closure.
    File { source: std::path::PathBuf },
    /// Symbol-level: `ctx.range` is always `Some(&range)` when the fetch
    /// closure runs; the builder short-circuits with empty data otherwise.
    Symbol {
        source: std::path::PathBuf,
        resolver: nyne_source::FragmentResolver,
        fragment_path: Arc<[String]>,
    },
}

/// Fetch context passed to template-data closures — rel path plus optional
/// symbol line range.
///
/// **Invariant:** `range` is always `Some` when the fetch closure is invoked
/// under [`FetchScope::Symbol`], and always `None` under [`FetchScope::File`].
/// [`build_read_fn`] enforces this by short-circuiting to empty data before
/// calling fetch when the symbol's range can no longer be resolved. Symbol-
/// only fetches may therefore `expect()` on `range`.
pub(crate) struct FetchCtx<'a> {
    pub rel: &'a str,
    pub range: Option<&'a nyne::SymbolLineRange>,
}

/// Build a read closure for `TemplateHandle::lazy_node` / `editable_lazy_node`.
///
/// Resolves `rel_path` (and optionally the symbol range) per render, then
/// delegates to `fetch` for the git query. Symbol scopes emit empty data
/// when the symbol has been removed from the source.
pub(crate) fn build_read_fn<F>(
    repo: Arc<Repo>,
    scope: FetchScope,
    fetch: F,
) -> impl Fn(&TemplateEngine, &str) -> Result<Vec<u8>> + Send + Sync + 'static
where
    F: Fn(&Repo, &FetchCtx<'_>) -> Result<minijinja::Value> + Send + Sync + 'static,
{
    move |engine: &TemplateEngine, tmpl: &str| match &scope {
        FetchScope::File { source } => {
            let rel = repo.rel_path(source);
            let data = fetch(&repo, &FetchCtx { rel: &rel, range: None })?;
            Ok(engine.render_bytes(tmpl, &data))
        }
        FetchScope::Symbol { source, resolver, fragment_path } => {
            let Some(range) = resolver.line_range(fragment_path)? else {
                return Ok(engine.render_bytes(tmpl, &minijinja::context!(data => Vec::<()>::new())));
            };
            let rel = repo.rel_path(source);
            let data = fetch(&repo, &FetchCtx { rel: &rel, range: Some(&range) })?;
            Ok(engine.render_bytes(tmpl, &data))
        }
    }
}
