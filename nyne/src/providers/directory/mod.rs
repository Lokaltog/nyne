//! Directory provider — lists contents and provides metadata.

use std::sync::Arc;

use nyne_macros::routes;
use serde::Serialize;

use super::names::{self, FILE_OVERVIEW};
use super::prelude::*;
use crate::dispatch::invalidation::InvalidationEvent;
use crate::dispatch::routing::ctx::RouteCtx;
use crate::dispatch::routing::tree::RouteTree;
use crate::templates::{TemplateHandle, serialize_view};
use crate::types::file_kind::FileKind;
use crate::types::path_conventions::split_companion_path;

const TMPL_OVERVIEW: &str = "directory/overview";

pub(super) struct DirectoryProvider {
    overview: TemplateHandle,
    routes: RouteTree<Self>,
}

impl DirectoryProvider {
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

        Ok(Some(vec![self.overview.node(FILE_OVERVIEW, serialize_view(view))]))
    }
}

/// A file entry for the directory overview template.
#[derive(Serialize)]
struct FileEntry {
    name: String,
    bytes: usize,
    description: String,
}

/// View model for the directory overview template.
#[derive(Serialize)]
struct DirOverviewView {
    dir_name: String,
    files: Vec<FileEntry>,
    subdirs: Vec<String>,
}

impl Provider for DirectoryProvider {
    fn id(&self) -> ProviderId { Self::PROVIDER_ID }

    fn children(self: Arc<Self>, ctx: &RequestContext<'_>) -> Nodes {
        let Some(split) = split_companion_path(ctx.path) else {
            return Ok(None);
        };
        super::companion_children(&self.routes, &self, ctx, &split)
    }

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

impl DirectoryProvider {
    pub(super) const PROVIDER_ID: ProviderId = ProviderId::new("directory");
}

#[cfg(test)]
mod tests;
