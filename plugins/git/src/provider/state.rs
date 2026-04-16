use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::router::{NamedNode, Request, WriteContext};
use nyne::templates::{HandleBuilder, TemplateEngine, TemplateGlobals, TemplateHandle};
use nyne::{SliceSpec, parse_spec};
use nyne_companion::CompanionRequest;

use super::views;
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
        fetch: impl Fn(&Repo, &str) -> Result<minijinja::Value> + Send + Sync + 'static,
    ) -> Option<NamedNode> {
        let source = req.source_file()?;
        let repo = Arc::clone(&self.repo);
        Some(handle.lazy_node(name, move |engine: &TemplateEngine, tmpl: &str| {
            let rel = repo.rel_path(&source);
            let data = fetch(&repo, &rel)?;
            Ok(engine.render_bytes(tmpl, &data))
        }))
    }

    /// Create an editable template node scoped to the request's source file.
    pub(crate) fn editable_file_content(
        &self,
        req: &Request,
        handle: &TemplateHandle,
        name: impl Into<String>,
        fetch: impl Fn(&Repo, &str) -> Result<minijinja::Value> + Send + Sync + 'static,
        write_fn: impl Fn(&Repo, &str, &[u8]) -> Result<()> + Send + Sync + 'static,
    ) -> Option<NamedNode> {
        let source = req.source_file()?;
        let repo = Arc::clone(&self.repo);
        let write_repo = Arc::clone(&self.repo);
        let write_source = source.clone();
        Some(handle.editable_lazy_node(
            name,
            move |engine: &TemplateEngine, tmpl: &str| {
                let rel = repo.rel_path(&source);
                let data = fetch(&repo, &rel)?;
                Ok(engine.render_bytes(tmpl, &data))
            },
            move |_ctx: &WriteContext<'_>, data: &[u8]| {
                let rel = write_repo.rel_path(&write_source);
                write_fn(&write_repo, &rel, data)?;
                Ok(vec![write_source.clone()])
            },
        ))
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
}
