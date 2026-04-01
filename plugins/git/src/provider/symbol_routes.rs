//! Symbol-scoped git features — per-symbol blame, log, and history.
//!
//! Registers into [`SourceExtensions::fragment_path`] during plugin
//! activation, contributing a `git/` subdirectory to each decomposed
//! symbol's companion namespace with blame, log, and historical versions
//! filtered to that symbol's line range.
//!
//! Extension callbacks inspect the captured `path` segments from the
//! `rest("path")` route to determine scope: symbol root (contribute
//! `git/` entry), `git/` directory (emit blame/log/history), or
//! `git/history/` (emit historical versions). This callback-based
//! approach is necessary because structural `dir()`/`content()` entries
//! inside a `rest()` route are never dispatched into — `rest` captures
//! all remaining segments and calls `handle_here`, which only auto-emits
//! static dir entries unconditionally but never recurses into them.

use std::path::Path;
use std::sync::Arc;

use color_eyre::eyre::{Result, eyre};
use nyne::SymbolLineRange;
use nyne::router::{NamedNode, Node, Request, RouteCtx};
use nyne::templates::{LazyView, TemplateEngine, TemplateHandle};
use nyne_companion::CompanionRequest;
use nyne_source::{DecompositionCache, FragmentResolver, SourceExtensions, SyntaxRegistry};

use super::{GitState, views};
use crate::history::{self, HistoryQueries as _, SymbolExtractCtx, filter_blame_to_range};
use crate::repo::Repo;

/// Default decomposition depth for extracting symbols from historical blobs.
const EXTRACT_MAX_DEPTH: usize = 5;

/// Shared context for symbol-scoped git extension callbacks.
///
/// Bundles the git state, decomposition cache, and syntax registry needed
/// by all symbol-level git callbacks. Captured as `Arc<SymbolGitCtx>` in
/// the `on_readdir`/`on_lookup` closures registered into
/// [`SourceExtensions::fragment_path`].
struct SymbolGitCtx {
    state: Arc<GitState>,
    decomposition: DecompositionCache,
    syntax: Arc<SyntaxRegistry>,
}

impl SymbolGitCtx {
    /// Build a [`FragmentResolver`] for the given source file.
    fn resolver(&self, source_file: &Path) -> FragmentResolver {
        FragmentResolver::new(self.decomposition.clone(), source_file.to_owned())
    }

    /// Convert borrowed path segments to an owned fragment path.
    fn to_fragment_path(segments: &[&str]) -> Arc<[String]> {
        Arc::from(segments.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
    }

    /// Create a lazy template node for a symbol-scoped git feature.
    ///
    /// Resolves the symbol's line range lazily at render time via
    /// [`FragmentResolver`], then delegates to `fetch` for the git query.
    /// Returns empty data when the symbol no longer exists in the source.
    fn lazy_template(
        &self,
        sf: &Path,
        symbol_segs: &[&str],
        handle: &TemplateHandle,
        name: &str,
        fetch: impl Fn(&Repo, &str, &SymbolLineRange) -> Result<minijinja::Value> + Send + Sync + 'static,
    ) -> NamedNode {
        let fragment_path = Self::to_fragment_path(symbol_segs);
        let resolver = self.resolver(sf);
        let repo = Arc::clone(&self.state.repo);
        let sf = sf.to_owned();
        handle.lazy_node(name, move |engine: &TemplateEngine, tmpl: &str| {
            let Some(range) = resolver.line_range(&fragment_path)? else {
                return Ok(engine.render_bytes(tmpl, &minijinja::context!(data => Vec::<()>::new())));
            };
            let data = fetch(&repo, &repo.rel_path(&sf), &range)?;
            Ok(engine.render_bytes(tmpl, &data))
        })
    }

    /// Build a blame node for a symbol's line range.
    fn blame_node(&self, sf: &Path, symbol_segs: &[&str]) -> NamedNode {
        self.lazy_template(
            sf,
            symbol_segs,
            &self.state.handles.blame,
            &self.state.vfs.file.blame,
            |repo, rel, range| Ok(minijinja::context!(data => filter_blame_to_range(repo.blame(rel)?, range))),
        )
    }

    /// Build a log node for a symbol's line range.
    fn log_node(&self, sf: &Path, symbol_segs: &[&str]) -> NamedNode {
        let limit = self.state.history_limit;
        self.lazy_template(
            sf,
            symbol_segs,
            &self.state.handles.log,
            &self.state.vfs.file.log,
            move |repo, rel, range| Ok(minijinja::context!(data => repo.file_history_in_range(rel, range, limit)?)),
        )
    }

    /// Readdir for `git/` — emit BLAME.md, LOG.md, and `history/`.
    #[allow(clippy::unnecessary_wraps)]
    fn readdir_git(&self, req: &mut Request, symbol_segs: &[&str]) -> Result<()> {
        let Some(sf) = req.source_file() else {
            return Ok(());
        };
        req.nodes.add(self.blame_node(&sf, symbol_segs));
        req.nodes.add(self.log_node(&sf, symbol_segs));
        req.nodes.add(NamedNode::dir(&self.state.vfs.dir.history));
        Ok(())
    }

    /// Lookup within `git/` — resolve individual files or sliced views.
    fn lookup_git(&self, req: &mut Request, symbol_segs: &[&str], name: &str) -> Result<()> {
        let Some(sf) = req.source_file() else {
            return Ok(());
        };
        if name == self.state.vfs.file.blame {
            req.nodes.add(self.blame_node(&sf, symbol_segs));
            return Ok(());
        }
        if name == self.state.vfs.file.log {
            req.nodes.add(self.log_node(&sf, symbol_segs));
            return Ok(());
        }
        if name == self.state.vfs.dir.history {
            req.nodes.add(NamedNode::dir(&self.state.vfs.dir.history));
            return Ok(());
        }
        // BLAME.md:{spec} or LOG.md:{spec}
        self.lookup_sliced_view(req, symbol_segs, name)
    }

    /// Resolve a sliced view lookup (`BLAME.md:{spec}` or `LOG.md:{spec}`).
    fn lookup_sliced_view(&self, req: &mut Request, symbol_segs: &[&str], name: &str) -> Result<()> {
        let Some((handle, spec, is_blame)) = self.state.resolve_sliced_view(name) else {
            return Ok(());
        };
        let sf = req.source_file().ok_or_else(|| eyre!("no source file"))?;
        let fragment_path = Self::to_fragment_path(symbol_segs);
        let resolver = self.resolver(&sf);
        let repo = Arc::clone(&self.state.repo);
        let limit = self.state.history_limit;
        let node = handle.named_node(
            name,
            LazyView::new(move |engine: &TemplateEngine, tmpl: &str| {
                let Some(range) = resolver.line_range(&fragment_path)? else {
                    return Ok(engine.render_bytes(tmpl, &minijinja::context!(data => Vec::<()>::new())));
                };
                let rel = repo.rel_path(&sf);
                let data = if is_blame {
                    minijinja::context!(data => spec.apply(&filter_blame_to_range(repo.blame(&rel)?, &range)))
                } else {
                    minijinja::context!(data => spec.apply(&repo.file_history_in_range(&rel, &range, limit)?))
                };
                Ok(engine.render_bytes(tmpl, &data))
            }),
        );
        req.nodes.add(node);
        Ok(())
    }

    /// Build history version nodes for a symbol, optionally filtering to a single name.
    fn history_nodes(&self, req: &mut Request, symbol_segs: &[&str], filter_name: Option<&str>) -> Result<()> {
        let sf = req.source_file().ok_or_else(|| eyre!("no source file"))?;
        let fragment_path = Self::to_fragment_path(symbol_segs);
        let Some(range) = self.resolver(&sf).line_range(&fragment_path)? else {
            return Ok(());
        };
        let repo = Arc::clone(&self.state.repo);
        let rel = repo.rel_path(&sf);
        let file_ext = sf.extension().and_then(|e| e.to_str()).unwrap_or("");
        let sym_ctx = Arc::new(SymbolExtractCtx {
            syntax: Arc::clone(&self.syntax),
            ext: file_ext.to_owned(),
            fragment_path,
            max_depth: EXTRACT_MAX_DEPTH,
        });
        let entries = repo.file_history_in_range(&rel, &range, self.state.history_limit)?;
        let rel: Arc<str> = Arc::from(rel);
        for (i, entry) in entries.into_iter().enumerate() {
            let filename = views::history_filename(i, &entry, file_ext);
            if filter_name.is_some_and(|n| n != filename) {
                continue;
            }
            let node = Node::file()
                .with_readable(history::HistoryVersionContent {
                    repo: Arc::clone(&repo),
                    rel_path: Arc::clone(&rel),
                    oid: entry.oid,
                    symbol_ctx: Some(Arc::clone(&sym_ctx)),
                })
                .named(filename);
            req.nodes.add(node);
            if filter_name.is_some() {
                return Ok(());
            }
        }
        Ok(())
    }
}

/// Register symbol-scoped git content into [`SourceExtensions`].
///
/// Contributes a `git/` directory to each symbol's companion namespace
/// containing blame, log, and historical versions scoped to the symbol's
/// line range. Uses `on_readdir`/`on_lookup` callbacks that inspect the
/// captured `path` segments to determine scope, following the same pattern
/// as the source plugin's `code/` and `edit/` sub-routes.
pub fn register_source_extensions(
    exts: &mut SourceExtensions,
    state: &Arc<GitState>,
    decomposition: &DecompositionCache,
    syntax: &Arc<SyntaxRegistry>,
) {
    let c = Arc::new(SymbolGitCtx {
        state: Arc::clone(state),
        decomposition: decomposition.clone(),
        syntax: Arc::clone(syntax),
    });

    exts.fragment_path.scoped("git", |ext| {
        let s = Arc::clone(&c);
        ext.on_readdir(move |ctx: &RouteCtx, req: &mut Request| {
            let Some(segments) = path_segments(ctx) else {
                return Ok(());
            };
            match classify(&segments, &s.state.vfs.dir.git, &s.state.vfs.dir.history) {
                GitScope::History(symbol_segs) => s.history_nodes(req, symbol_segs, None),
                GitScope::GitDir(symbol_segs) => s.readdir_git(req, symbol_segs),
                GitScope::SymbolRoot => {
                    let Some(sf) = req.source_file() else {
                        return Ok(());
                    };
                    if s.decomposition.has_fragment(&sf, &to_owned(&segments)) {
                        req.nodes.add(NamedNode::dir(&s.state.vfs.dir.git));
                    }
                    Ok(())
                }
            }
        });

        let s = Arc::clone(&c);
        ext.on_lookup(move |ctx: &RouteCtx, req: &mut Request, name: &str| {
            let Some(segments) = path_segments(ctx) else {
                return Ok(());
            };
            match classify(&segments, &s.state.vfs.dir.git, &s.state.vfs.dir.history) {
                GitScope::History(symbol_segs) => s.history_nodes(req, symbol_segs, Some(name)),
                GitScope::GitDir(symbol_segs) => s.lookup_git(req, symbol_segs, name),
                GitScope::SymbolRoot => {
                    if name == s.state.vfs.dir.git {
                        let Some(sf) = req.source_file() else {
                            return Ok(());
                        };
                        if s.decomposition.has_fragment(&sf, &to_owned(&segments)) {
                            req.nodes.add(NamedNode::dir(&s.state.vfs.dir.git));
                        }
                    }
                    Ok(())
                }
            }
        });
    });
}

/// Extract path segments from the route context's `path` parameter.
///
/// Returns `None` when the parameter is absent or empty (e.g. when
/// `dispatch_into_rest` fires for the symbols root level).
fn path_segments(ctx: &RouteCtx) -> Option<Vec<&str>> {
    let path = ctx.param("path")?;
    if path.is_empty() {
        return None;
    }
    Some(path.split('/').collect())
}

/// Which part of the `git/` sub-route the captured path resolves to.
enum GitScope<'a> {
    /// Path ends with `git/history/` — symbol segments are the prefix.
    History(&'a [&'a str]),
    /// Path ends with `git/` — symbol segments are the prefix.
    GitDir(&'a [&'a str]),
    /// Path is a plain symbol directory — no git suffix.
    SymbolRoot,
}

/// Classify captured path segments relative to the git sub-route.
fn classify<'a>(segments: &'a [&'a str], git_dir: &str, history_dir: &str) -> GitScope<'a> {
    if let Some((last, parent)) = segments.split_last()
        && *last == history_dir
        && let Some((git, symbol_segs)) = parent.split_last()
        && *git == git_dir
        && !symbol_segs.is_empty()
    {
        return GitScope::History(symbol_segs);
    }
    if let Some((last, symbol_segs)) = segments.split_last()
        && *last == git_dir
        && !symbol_segs.is_empty()
    {
        return GitScope::GitDir(symbol_segs);
    }
    GitScope::SymbolRoot
}

/// Convert borrowed path segments to owned for [`DecompositionCache::has_fragment`].
fn to_owned(segments: &[&str]) -> Vec<String> { segments.iter().map(|s| (*s).to_owned()).collect() }
