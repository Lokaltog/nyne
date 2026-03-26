//! TODO/FIXME provider — scans source files for TODO markers.

/// TODO entry -- a single TODO marker in source code.
mod entry;
/// TODO scanner -- Aho-Corasick automaton for tag detection.
mod scan;

use std::collections::{BTreeMap, HashMap, HashSet};

use entry::TodoEntry;
use nyne::dispatch::routing::ctx::RouteCtx;
use nyne::dispatch::routing::tree::RouteTree;
use nyne::prelude::*;
use nyne::templates::{TemplateHandle, serialize_view};
use nyne_macros::routes;
use nyne_source::SyntaxRegistry;
use parking_lot::RwLock;
use scan::TodoScanner;
use serde::Serialize;

use crate::config::Config;

/// TODO directory name at the `@/` root level.
const DIR_TODO: &str = "todo";

/// TODO provider — aggregates TODO/FIXME markers from source files.
///
/// Exposes `@/todo/` containing per-tag subdirectories (e.g. `TODO/`, `FIXME/`)
/// with symlinks pointing back to the source file and line. Scans lazily on
/// first access using the git index as the file list, and invalidates the
/// cached index when scanned files change on disk.
pub struct TodoProvider {
    ctx: Arc<ActivationContext>,
    config: Config,
    syntax: Arc<SyntaxRegistry>,
    scanner: TodoScanner,
    index: RwLock<Option<TodoIndex>>,
    overview_tmpl: TemplateHandle,
    tag_tmpl: TemplateHandle,
    /// Canonical tag list from config (SSOT for priority order).
    tags: Vec<String>,
    routes: RouteTree<Self>,
}

/// Cached scan results.
///
/// Built lazily on first access by scanning all git-tracked files.
/// Invalidated via `on_fs_change` when any of the `scanned_files` is
/// modified, causing a full re-scan on the next read.
struct TodoIndex {
    /// All discovered entries, grouped by tag.
    entries_by_tag: BTreeMap<String, Vec<TodoEntry>>,
    /// Set of files that were scanned (for invalidation).
    scanned_files: HashSet<VfsPath>,
}

/// Methods for [`TodoProvider`].
impl TodoProvider {
    /// Create a new TODO provider with route tree and scanner.
    pub(crate) fn new(ctx: Arc<ActivationContext>, config: Config) -> Self {
        let tags = config.tags.clone();
        let scanner = TodoScanner::new(&tags);
        let syntax = SyntaxRegistry::global();

        let mut b = nyne::handle_builder();
        let overview_key = b.register("todo/overview", include_str!("templates/overview.md.j2"));
        let tag_key = b.register("todo/tag", include_str!("templates/tag.md.j2"));
        let engine = b.finish();
        let overview_tmpl = TemplateHandle::new(&engine, overview_key);
        let tag_tmpl = TemplateHandle::new(&engine, tag_key);

        let routes = routes!(Self, {
            no_emit "@" => children_at_root {
                "todo" => children_todo_root {
                    "{tag}" => children_tag_dir,
                }
            }
        });

        Self {
            ctx,
            config,
            syntax,
            scanner,
            index: RwLock::new(None),
            overview_tmpl,
            tag_tmpl,
            tags,
            routes,
        }
    }

    /// Ensure the index is populated, scanning lazily on first access.
    fn ensure_index(&self, ctx: &RequestContext<'_>) {
        // Fast path: already populated.
        if self.index.read().is_some() {
            return;
        }

        let mut index = self.index.write();
        // Double-check after acquiring write lock — another thread may have
        // populated the index while we waited.
        if index.is_some() {
            return;
        }

        // Discover and scan inside the write lock to avoid redundant work
        // when multiple threads race past the read-lock fast path.
        let files = self.discover_files();
        let entries_by_tag = self.scanner.scan_all(&files, ctx.real_fs, &self.syntax);
        *index = Some(TodoIndex {
            entries_by_tag,
            scanned_files: files.into_iter().collect(),
        });
    }

    /// Get the list of files to scan from the git index.
    #[cfg(feature = "git")]
    fn discover_files(&self) -> Vec<VfsPath> {
        let Some(repo) = self.ctx.get::<Arc<nyne_git::GitRepo>>() else {
            return Vec::new();
        };
        let Ok(paths) = repo.index_paths() else {
            return Vec::new();
        };

        let syntax = &self.syntax;
        paths
            .into_iter()
            .filter_map(|p| {
                let vpath = VfsPath::new(&p).ok()?;
                let ext = vpath.extension()?;
                // Only include files with a registered decomposer.
                syntax.get(ext)?;
                Some(vpath)
            })
            .collect()
    }

    /// Returns an empty list when the git feature is disabled.
    #[cfg(not(feature = "git"))]
    fn discover_files(&self) -> Vec<VfsPath> { Vec::new() }

    /// At `@/` level — contribute the "todo" directory.
    #[expect(clippy::unused_self, reason = "route handler called as instance method")]
    fn children_at_root(&self, _ctx: &RouteCtx<'_>) -> Vec<VirtualNode> { vec![VirtualNode::directory(DIR_TODO)] }

    /// At `@/todo/` level — list tag dirs + overview.
    #[allow(clippy::unnecessary_wraps)] // matches Provider::children return type
    /// List children at the todo root: tag directories and overview.
    fn children_todo_root(&self, ctx: &RouteCtx<'_>) -> Nodes {
        self.ensure_index(ctx);
        let index_guard = self.index.read();
        let Some(index) = index_guard.as_ref() else {
            return Ok(Some(Vec::new()));
        };

        let mut nodes = Vec::new();

        // OVERVIEW.md — all tags, grouped by file, ranked by priority.
        let overview_view = build_overview_view(index, &self.tags);
        nodes.push(self.overview_tmpl.node("OVERVIEW.md", serialize_view(&overview_view)));

        // Per-tag: directory + .md file.
        for tag in &self.tags {
            let Some(tag_entries) = index.entries_by_tag.get(tag.as_str()).filter(|e| !e.is_empty()) else {
                continue;
            };
            nodes.push(VirtualNode::directory(tag));
            let tag_view = build_tag_view(tag, tag_entries);
            nodes.push(self.tag_tmpl.node(format!("{tag}.md"), serialize_view(&tag_view)));
        }

        Ok(Some(nodes))
    }

    /// At `@/todo/<TAG>/` level — list entries for a specific tag.
    fn children_tag_dir(&self, ctx: &RouteCtx<'_>) -> Vec<VirtualNode> {
        let tag = ctx.param("tag");
        self.ensure_index(ctx);
        let index_guard = self.index.read();
        let Some(index) = index_guard.as_ref() else {
            return Vec::new();
        };

        let Some(entries) = index.entries_by_tag.get(tag) else {
            return Vec::new();
        };

        entries
            .iter()
            .map(|e| VirtualNode::symlink(e.fs_name(), e.symlink_target()))
            .collect()
    }
}

/// [`Provider`] implementation for [`TodoProvider`].
impl Provider for TodoProvider {
    /// Return the TODO provider identifier.
    fn id(&self) -> ProviderId { Self::PROVIDER_ID }

    /// Activate when the workspace is a git repository.
    fn should_activate(&self, _ctx: &ActivationContext) -> bool {
        if !self.config.enabled {
            return false;
        }
        #[cfg(feature = "git")]
        {
            self.ctx.get::<Arc<nyne_git::GitRepo>>().is_some()
        }
        #[cfg(not(feature = "git"))]
        {
            false
        }
    }

    /// Dispatch children through the route tree.
    fn children(self: Arc<Self>, ctx: &RequestContext<'_>) -> Nodes { self.routes.children(&self, ctx) }

    #[allow(clippy::expect_used)] // static path constant, always valid
    /// Invalidate TODO index when scanned files change.
    fn on_fs_change(&self, changed: &[VfsPath]) -> Vec<InvalidationEvent> {
        let index_guard = self.index.read();
        if let Some(idx) = index_guard.as_ref() {
            let dominated = changed.iter().any(|p| idx.scanned_files.contains(p));
            if dominated {
                drop(index_guard);
                // Invalidate entire index — next access triggers rescan.
                *self.index.write() = None;
                return vec![InvalidationEvent::Subtree {
                    path: VfsPath::new(&format!("@/{DIR_TODO}")).expect("static todo path is valid"),
                }];
            }
        }
        Vec::new()
    }
}

/// Provider ID constant.
impl TodoProvider {
    /// Unique provider identifier for TODO scanning.
    pub(crate) const PROVIDER_ID: ProviderId = ProviderId::new("todo");
}

/// Overview view — all TODO entries grouped by tag and file.
#[derive(Serialize)]
struct OverviewView {
    files: Vec<FileGroup>,
}

/// Group of entries from a single file.
#[derive(Serialize)]
struct FileGroup {
    path: String,
    entries: Vec<EntryView>,
}

/// Serializable TODO entry view.
#[derive(Serialize)]
struct EntryView {
    line: usize,
    tag: String,
    text: String,
}

impl From<&TodoEntry> for EntryView {
    fn from(entry: &TodoEntry) -> Self {
        Self {
            line: entry.line,
            tag: entry.tag.to_string(),
            text: entry.text.clone(),
        }
    }
}

/// Tag view — all entries for a specific tag.
#[derive(Serialize)]
struct TagView {
    tag: String,
    files: Vec<FileGroup>,
}

/// Group sorted entries by source file path.
///
/// Entries must already be sorted with file path as the primary key —
/// consecutive entries from the same file are merged into one `FileGroup`.
fn group_by_file(entries: &[&TodoEntry]) -> Vec<FileGroup> {
    entries
        .chunk_by(|a, b| a.source_file == b.source_file)
        .filter_map(|chunk| {
            let first = chunk.first()?;
            Some(FileGroup {
                path: first.source_file.as_str().to_owned(),
                entries: chunk.iter().map(|e| EntryView::from(*e)).collect(),
            })
        })
        .collect()
}

/// Build the overview view: all tags, grouped by file, entries sorted by
/// priority and line number.
fn build_overview_view(index: &TodoIndex, tag_order: &[String]) -> OverviewView {
    // Build a priority map from tag order (SSOT).
    let priority: HashMap<&str, usize> = tag_order.iter().enumerate().map(|(i, t)| (t.as_str(), i)).collect();

    // Collect all entries, flatten across tags.
    let mut all_entries: Vec<&TodoEntry> = index.entries_by_tag.values().flat_map(|v| v.iter()).collect();

    // Sort by file path, then by tag priority, then by line number.
    all_entries.sort_by(|a, b| {
        a.source_file
            .as_str()
            .cmp(b.source_file.as_str())
            .then_with(|| {
                let pa = priority.get(&*a.tag).copied().unwrap_or(usize::MAX);
                let pb = priority.get(&*b.tag).copied().unwrap_or(usize::MAX);
                pa.cmp(&pb)
            })
            .then_with(|| a.line.cmp(&b.line))
    });

    OverviewView {
        files: group_by_file(&all_entries),
    }
}

/// Build a per-tag view: entries for a single tag, grouped by file.
fn build_tag_view(tag: &str, entries: &[TodoEntry]) -> TagView {
    let mut sorted: Vec<&TodoEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| {
        a.source_file
            .as_str()
            .cmp(b.source_file.as_str())
            .then_with(|| a.line.cmp(&b.line))
    });

    TagView {
        tag: tag.to_owned(),
        files: group_by_file(&sorted),
    }
}
