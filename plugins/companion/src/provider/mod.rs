mod state;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::path_filter::PathFilter;
use nyne::path_utils::PathExt;
use nyne::router::fs::Filesystem;
use nyne::router::{
    AffectedFiles, InvalidationEvent, NamedNode, Next, NodeKind, Op, Provider, RenameContext, Renameable, Request,
    RouteTree,
};
use nyne_visibility::{Visibility, VisibilityRequest};
pub use state::*;
/// Path-rewriting middleware and extension tree owner for the companion
/// namespace.
///
/// Strips the configurable companion suffix, sets `Companion`
/// state, rewrites op with clean names, and dispatches through the
/// appropriate extension tree before forwarding to the chain.
///
/// Three trees exist:
/// - **`file_tree`** — per-file companion content (`file.rs@/`)
/// - **`dir_tree`** — per-directory companion content (`dir@/`)
/// - **`mount_tree`** — mount-wide companion content (`./@/`)
///
/// Dispatch selects the tree based on [`Companion::source_file`] and
/// filesystem kind: files → `file_tree`, directories → `dir_tree`,
/// mount root (`None`) → `mount_tree`.
pub struct CompanionProvider {
    pub(crate) suffix: Arc<str>,
    pub(crate) file_tree: RouteTree<Self>,
    pub(crate) dir_tree: RouteTree<Self>,
    pub(crate) mount_tree: RouteTree<Self>,
    pub(crate) fs: Arc<dyn Filesystem>,
    /// Gitignore-backed filter consulted to bypass companion decoration
    /// for ignored paths. `None` means no filtering (used in tests and
    /// minimal chains).
    pub(crate) path_filter: Option<Arc<PathFilter>>,
}

nyne::define_provider!(CompanionProvider, "companion");

impl CompanionProvider {
    /// Returns `true` if `path` is gitignored (or otherwise excluded
    /// via `excluded_patterns`) and companion decoration should be
    /// skipped so the request passes through to the underlying
    /// filesystem untouched.
    fn is_excluded(&self, path: &Path) -> bool { self.path_filter.as_ref().is_some_and(|f| f.is_excluded(path)) }

    /// Select the extension tree based on companion scope.
    ///
    /// File-backed companions dispatch through `file_tree`, directory
    /// companions through `dir_tree`, and the mount root (bare `@`)
    /// through `mount_tree`.
    fn tree_for(&self, req: &Request) -> &RouteTree<Self> {
        let Some(source_file) = req.companion().and_then(|c| c.source_file.as_ref()) else {
            return &self.mount_tree;
        };
        let parent = source_file.parent().unwrap_or_else(|| Path::new(""));
        let is_dir = source_file
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|name| self.fs.stat(parent, name).ok().flatten())
            .is_some_and(|entry| entry.kind == NodeKind::Directory);
        if is_dir { &self.dir_tree } else { &self.file_tree }
    }

    /// Detect and rewrite companion `@` suffixes in the request path.
    fn rewrite_companion_path(&self, req: &mut Request) {
        let path = req.path().to_path_buf();
        let components: Vec<&str> = path.iter().filter_map(|c| c.to_str()).collect();
        let Some(idx) = components.iter().position(|c| c.ends_with(&*self.suffix)) else {
            return;
        };
        let Some(&suffixed) = components.get(idx) else {
            return;
        };
        let real_name = suffixed.strip_suffix(&*self.suffix).unwrap_or(suffixed);
        let parent: PathBuf = components.get(..idx).unwrap_or_default().iter().collect();
        let source_file = match real_name {
            "" if parent.as_os_str().is_empty() => None,
            "" => Some(parent),
            _ => Some(parent.join(real_name)),
        };
        // Skip the rewrite when the resolved source file is gitignored.
        // Leave `Companion` state unset so the rest of the chain treats
        // the request as a plain real-fs access; downstream middlewares
        // (source, lsp, git, …) no-op on their `req.companion().is_none()`
        // guards.
        if let Some(ref src) = source_file
            && self.is_excluded(src)
        {
            return;
        }
        let rest: PathBuf = components
            .get(idx + 1..)
            .unwrap_or_default()
            .iter()
            .map(|c| c.strip_suffix(&*self.suffix).unwrap_or(c))
            .collect();
        tracing::trace!("companion:path_rewrite:{}->{}", path.display(), rest.display());
        req.set_state(Companion::new(source_file, Arc::clone(&self.suffix)));
        req.rewrite_path(rest);
    }

    /// Handle companion lookup operations.
    ///
    /// Three cases:
    /// 1. **No companion suffix** — dispatch through extension tree if in
    ///    companion context (extensions may contribute dirs/content),
    ///    otherwise pass through.
    /// 2. **Inner `@`** (already in companion context) — strip suffix from
    ///    lookup name, forward to chain unchanged.
    /// 3. **Outer `@`** (entering companion context) — set companion state,
    ///    rewrite path, dispatch through extension tree.
    fn accept_lookup(&self, req: &mut Request, next: &Next, name: &str) -> Result<()> {
        let Some(real_name) = name.strip_suffix(&*self.suffix) else {
            // Case 1: no companion suffix. Dispatch through extension tree
            // if in companion context (tree falls through to next on no match),
            // otherwise pass through directly.
            if req.companion().is_some() {
                return self.tree_for(req).dispatch(self, req, next);
            }
            return next.run(req);
        };
        // Case 2: inner `@` suffixes (e.g., `Foo@` inside `symbols/`) strip
        // the `@` from the lookup name but do NOT reset the path. The
        // downstream route tree's capture will match the clean name.
        if req.companion().is_some() {
            tracing::trace!("companion:inner_rewrite:{name}->{real_name}");
            req.set_op(Op::Lookup {
                name: real_name.to_owned(),
            });
            req.nodes.add(NamedNode::dir(name));
            return next.run(req);
        }
        // Case 3: outer `@` — entering companion context.
        let source_file = req.path().join(real_name);
        let source_file = (!source_file.as_os_str().is_empty()).then_some(source_file);
        // Skip the rewrite for gitignored source files. Without
        // `Companion` state or an emitted `@` dir, the lookup falls
        // through to `next.run(req)` below and the terminal filesystem
        // provider returns `ENOENT` for the literal `<name>@` path.
        if let Some(ref src) = source_file
            && self.is_excluded(src)
        {
            tracing::trace!("companion:lookup_excluded:{}", src.display());
            return next.run(req);
        }
        tracing::trace!("companion:rewrite:{name}->{real_name}");

        req.set_state(Companion::new(source_file, Arc::clone(&self.suffix)));

        req.nodes.add(NamedNode::dir(name));
        req.rewrite_path(PathBuf::new());
        req.set_op(Op::Lookup {
            name: real_name.to_owned(),
        });
        self.tree_for(req).dispatch(self, req, next)
    }

    /// Handle companion rename operations.
    fn accept_rename(
        &self,
        req: &mut Request,
        next: &Next,
        src_name: &str,
        target_dir: &Path,
        target_name: &str,
    ) -> Result<()> {
        let Some(real_src) = src_name.strip_suffix(&*self.suffix) else {
            return next.run(req);
        };
        let real_tgt = target_name.strip_suffix(&*self.suffix).unwrap_or(target_name);

        tracing::trace!("companion:rewrite:{src_name}->{real_src}");

        req.set_state(Companion::new(
            Some(req.path().join(real_src)),
            Arc::clone(&self.suffix),
        ));
        req.set_op(Op::Rename {
            src_name: real_src.to_owned(),
            target_dir: target_dir.to_path_buf(),
            target_name: real_tgt.to_owned(),
        });
        next.run(req)
    }

    /// Emit companion directories for real files in readdir results.
    ///
    /// Only emitted for `Force` visibility (e.g. Claude Code) — normal
    /// processes see the real filesystem without companion dirs.
    /// Also skipped inside companion contexts — nested companion dirs
    /// (e.g., `CALLERS.md@` inside `mod.rs@/symbols/exec@/`) are only
    /// accessible via lookup, not listed in readdir.
    fn emit_companion_dirs(&self, req: &mut Request) {
        if req.companion().is_some() {
            return;
        }
        if !matches!(req.visibility(), Some(Visibility::Force)) {
            return;
        }
        let dir = req.path().to_path_buf();
        let file_names: Vec<String> = req
            .nodes
            .iter()
            .filter(|n| n.kind() == NodeKind::File)
            .map(|n| n.name().to_owned())
            .filter(|name| !self.is_excluded(&dir.join(name)))
            .collect();
        for name in file_names {
            req.nodes.add(NamedNode::dir(format!("{name}{}", self.suffix)));
        }
    }

    /// Inherit the source file's timestamps onto all virtual nodes that lack
    /// explicit timestamps.
    ///
    /// Called after dispatch so every plugin's contributed nodes are covered.
    fn stamp_source_timestamps(&self, req: &mut Request) {
        let Some(companion) = req.companion() else {
            return;
        };
        let Some(ref source_file) = companion.source_file else {
            return;
        };
        let Ok(meta) = self.fs.metadata(source_file) else {
            return;
        };
        for node in req.nodes.iter_mut() {
            node.set_default_timestamps(meta.timestamps);
        }
    }

    /// Wrap [`Renameable`] capabilities on accumulated nodes so they
    /// receive paths with the companion suffix stripped.
    fn wrap_renameables(&self, req: &mut Request) {
        for node in req.nodes.iter_mut() {
            if let Some(inner) = node.take_renameable() {
                node.set_renameable(CompanionRenameable {
                    inner,
                    suffix: Arc::clone(&self.suffix),
                });
            }
        }
    }
}
/// Decorator that strips the companion suffix from rename paths before
/// delegating to the inner [`Renameable`].
struct CompanionRenameable {
    inner: Arc<dyn Renameable>,
    suffix: Arc<str>,
}

impl Renameable for CompanionRenameable {
    fn rename(&self, ctx: &RenameContext<'_>) -> Result<AffectedFiles> {
        let clean_src = ctx.source.strip_name_suffix(&self.suffix);
        let clean_tgt = ctx.target.strip_name_suffix(&self.suffix);
        let clean_ctx = RenameContext {
            source: clean_src.as_deref().unwrap_or(ctx.source),
            target: clean_tgt.as_deref().unwrap_or(ctx.target),
        };
        self.inner.rename(&clean_ctx)
    }
}

impl Provider for CompanionProvider {
    fn accept(&self, req: &mut Request, next: &Next) -> Result<()> {
        if req.companion().is_none() {
            self.rewrite_companion_path(req);
        }
        // Inside companion context: force visibility so the
        // visibility post-filter doesn't strip virtual nodes.
        if req.companion().is_some() {
            req.set_state(Visibility::Force);
        }

        let result = match req.op().clone() {
            Op::Lookup { name } => self.accept_lookup(req, next, &name),
            Op::Rename {
                src_name,
                target_dir,
                target_name,
            } => self.accept_rename(req, next, &src_name, &target_dir, &target_name),
            Op::Readdir => {
                if req.companion().is_some() {
                    // In companion context — dispatch through extension tree,
                    // which calls next.run() internally at leaf nodes.
                    self.tree_for(req).dispatch(self, req, next)?;
                } else {
                    next.run(req)?;
                }
                self.emit_companion_dirs(req);
                Ok(())
            }
            _ =>
                if req.companion().is_some() {
                    self.tree_for(req).dispatch(self, req, next)
                } else {
                    next.run(req)
                },
        };

        // Wrap Renameable capabilities so they receive clean (suffix-stripped) paths.
        if req.companion().is_some() {
            self.wrap_renameables(req);
        }

        // Inherit source file mtime onto all virtual nodes that lack
        // explicit timestamps — covers nodes from every downstream plugin.
        self.stamp_source_timestamps(req);

        result
    }

    /// Expand source file changes to companion namespace invalidation events.
    fn on_change(&self, changed: &[PathBuf]) -> Vec<InvalidationEvent> {
        changed
            .iter()
            .filter_map(|p| {
                let name = p.file_name()?.to_str()?;
                let path = p
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .join(format!("{name}{}", self.suffix));
                Some(InvalidationEvent { path })
            })
            .collect()
    }
}
#[cfg(test)]
mod tests;
