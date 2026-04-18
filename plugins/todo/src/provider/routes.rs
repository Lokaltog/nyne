//! Extension registration for the TODO provider.
//!
//! ```text
//! todo/
//! ├── OVERVIEW.md          (content: overview template)
//! ├── {tag}.md             (content: per-tag template)
//! └── {tag}/               (capture)
//!     └── <symlink entries>
//! ```

use std::sync::Arc;

use nyne::router::{NamedNode, Node, Request, RouteCtx};
use nyne::templates::serialize_view;
use nyne_companion::{CompanionExtensions, CompanionRequest};

use super::state::TodoState;
use super::views::{build_overview_view, build_tag_view};

/// Register TODO routes into the mount-wide companion extension point.
#[allow(clippy::excessive_nesting)]
pub fn register_companion_extensions(exts: &mut CompanionExtensions, state: &Arc<TodoState>) {
    exts.mount.scoped("todo", |ext| {
        ext.dir(state.vfs.todo.clone(), |d| {
            // todo/ — emit a directory node per non-empty tag.
            let s = Arc::clone(state);
            d.on_readdir(move |_ctx: &RouteCtx, req: &mut Request| {
                s.ensure_index();
                let index_guard = s.index.read();
                let Some(index) = index_guard.as_ref() else {
                    return Ok(());
                };
                for tag in &s.tags {
                    if index.entries_by_tag.get(tag.as_str()).is_some_and(|e| !e.is_empty()) {
                        req.nodes.add(NamedNode::dir(tag));
                    }
                }
                Ok(())
            });

            // todo/OVERVIEW.md — all tags grouped by file.
            let s = Arc::clone(state);
            let file_overview = state.vfs.overview.clone();
            d.content(move |_ctx: &RouteCtx, _req: &Request| {
                s.ensure_index();
                let index_guard = s.index.read();
                let index = index_guard.as_ref()?;
                let view = build_overview_view(index, &s.tags);
                Some(s.overview_tmpl.named_node(&file_overview, serialize_view(&view)))
            });

            // todo/{tag}.md — entries for one tag, grouped by file.
            //
            // Registered per-tag (one content callback each) because the route
            // DSL has no `{tag}.md` capture at this scope — `ctx.param("tag")`
            // is only bound inside the sibling `capture("tag", ...)` subtree.
            // Auto-emit filters by name on lookup and emits all on readdir.
            for tag in &state.tags {
                let s = Arc::clone(state);
                let tag = tag.clone();
                d.content(move |_ctx: &RouteCtx, _req: &Request| {
                    s.ensure_index();
                    let index_guard = s.index.read();
                    let index = index_guard.as_ref()?;
                    let tag_entries = index.entries_by_tag.get(tag.as_str()).filter(|e| !e.is_empty())?;
                    let view = build_tag_view(&tag, tag_entries);
                    Some(s.tag_tmpl.named_node(format!("{tag}.md"), serialize_view(&view)))
                });
            }

            // todo/{tag}/ — capture for per-tag views and symlinks.
            let s = Arc::clone(state);
            d.capture("tag", move |d| {
                // todo/{tag}/ — symlinks back to source at-line for each entry.
                let s2 = Arc::clone(&s);
                d.on_readdir(move |ctx: &RouteCtx, req: &mut Request| {
                    let Some(tag) = ctx.param("tag") else {
                        return Ok(());
                    };
                    let Some(companion) = req.companion().cloned() else {
                        return Ok(());
                    };
                    s2.ensure_index();
                    let index_guard = s2.index.read();
                    let Some(index) = index_guard.as_ref() else {
                        return Ok(());
                    };
                    let Some(entries) = index.entries_by_tag.get(tag) else {
                        return Ok(());
                    };
                    for e in entries {
                        let target = e.symlink_target(&companion, &s2.vfs.todo, &s2.source_paths);
                        req.nodes.add(Node::symlink(target).named(e.fs_name()));
                    }
                    Ok(())
                });
            });
        });
    });
}
