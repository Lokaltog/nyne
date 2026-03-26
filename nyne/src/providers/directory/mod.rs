//! Directory provider — lists contents and provides metadata.

//! Directory overview provider -- generates `OVERVIEW.md` for directory companions.
//!
//! When a user reads `dir@/OVERVIEW.md`, this provider renders a summary of
//! the directory's contents (file listing with sizes, extensions, and token
//! estimates). The template is registered once at activation time and rendered
//! on each read with live filesystem data.

use nyne_macros::routes;
use serde::Serialize;

use super::names::{self, FILE_OVERVIEW};
use super::prelude::*;
use crate::dispatch::routing::ctx::RouteCtx;
use crate::dispatch::routing::tree::RouteTree;
use crate::templates::{TemplateHandle, serialize_view};
use crate::types::file_kind::FileKind;
use crate::types::path_conventions::split_companion_path;

/// Template key for the directory overview.
const TMPL_OVERVIEW: &str = "directory/overview";

/// Provider that generates OVERVIEW.md for directory companions.
pub(super) struct DirectoryProvider {
    overview: TemplateHandle,
    routes: RouteTree<Self>,
}

/// Construction and route handlers for the directory provider.
impl DirectoryProvider {
    /// Creates a new directory provider with the overview template.
    pub(super) fn new(_ctx: Arc<ActivationContext>) -> Self {
        let mut b = names::handle_builder();
        let overview_key = b.register(TMPL_OVERVIEW, include_str!("templates/overview.md.j2"));
        let engine = b.finish();
        let overview = TemplateHandle::new(&engine, overview_key);

        let routes = routes!(Self, {
            children(children_companion_root),
        });

        Self { overview, routes }
    }

    /// Generates OVERVIEW.md content for a directory companion.
    fn children_companion_root(&self, ctx: &RouteCtx<'_>) -> Nodes {
        let source = VfsPath::new(ctx.param("source"))?;

        // Only activate for directories.
        if !ctx.real_fs.is_dir(&source) {
            return Ok(None);
        }

        let entries = ctx.real_fs.read_dir(&source)?;

        let mut files = Vec::new();
        let mut subdirs = Vec::new();

        for entry in &entries {
            match entry.file_type {
                FileKind::Directory => {
                    subdirs.push(entry.name.clone());
                }
                FileKind::File => {
                    let file_path = source.join(&entry.name)?;
                    let bytes = ctx
                        .real_fs
                        .metadata(&file_path)
                        .map(|m| usize::try_from(m.size).unwrap_or(usize::MAX))
                        .unwrap_or(0);
                    let description = String::new();
                    files.push(FileEntry {
                        name: entry.name.clone(),
                        bytes,
                        description,
                    });
                }
                FileKind::Symlink => {}
            }
        }

        files.sort_by(|a, b| a.name.cmp(&b.name));
        subdirs.sort();

        let dir_name = source.name().unwrap_or(source.as_str()).to_owned();

        let view = DirOverviewView {
            dir_name,
            files,
            subdirs,
        };

        Ok(Some(vec![self.overview.node(FILE_OVERVIEW, serialize_view(&view))]))
    }
}

/// A file entry for the directory overview template.
///
/// Serialized into the Jinja template context so the overview can render
/// a table row per file with name, size, and a one-line description
/// (extracted from the first non-empty line of the file content).
#[derive(Serialize)]
struct FileEntry {
    name: String,
    bytes: usize,
    description: String,
}

/// View model for the directory overview template.
#[derive(Serialize)]
struct DirOverviewView {
    /// Name of the directory (last path component).
    dir_name: String,
    /// Regular files in the directory, sorted alphabetically.
    files: Vec<FileEntry>,
    /// Subdirectory names, sorted alphabetically.
    subdirs: Vec<String>,
}

/// Provider implementation for directory overview generation.
impl Provider for DirectoryProvider {
    /// Returns the directory provider identifier.
    fn id(&self) -> ProviderId { Self::PROVIDER_ID }

    /// Dispatches children through companion routing.
    fn children(self: Arc<Self>, ctx: &RequestContext<'_>) -> Nodes {
        let Some(split) = split_companion_path(ctx.path) else {
            return Ok(None);
        };
        super::companion_children(&self.routes, &self, ctx, &split)
    }

    /// Invalidates companion subtrees when real directory contents change.
    fn on_fs_change(&self, changed: &[VfsPath]) -> Vec<InvalidationEvent> {
        changed
            .iter()
            .filter_map(|p| {
                let parent = p.parent().unwrap_or(VfsPath::root());
                let dir_name = parent.name()?;
                let grandparent = parent.parent().unwrap_or(VfsPath::root());
                let companion = super::companion_name(dir_name);
                let companion_path = grandparent.join(&companion).ok()?;
                Some(InvalidationEvent::Subtree { path: companion_path })
            })
            .collect()
    }
}

/// Associated constants for the directory provider.
impl DirectoryProvider {
    /// The provider identifier for the directory provider.
    pub(super) const PROVIDER_ID: ProviderId = ProviderId::new("directory");
}

/// Unit tests.
#[cfg(test)]
mod tests;
