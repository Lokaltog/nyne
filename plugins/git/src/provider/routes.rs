//! Route tree and handler/content functions for the git provider.

use std::str::from_utf8;
use std::sync::Arc;
use std::time::Duration;

use color_eyre::eyre::{Result, WrapErr};
use nyne::router::{CachePolicy, Filesystem, NamedNode, Next, Node, Op, Request, RouteCtx};
use nyne_companion::{CompanionExtensions, CompanionRequest};

use super::branches::{branch_segments_at_prefix, branch_tree_nodes};
use super::diff::{DiffContent, DiffTarget};
use super::state::build_read_fn_file;
use super::status::StatusView;
use super::{GitFileRename, GitState, views};
use crate::history::HistoryQueries as _;
use crate::repo::Repo;

/// Register per-file companion content into [`CompanionExtensions`].
///
/// Contributes `git/`, `diff/`, and `history/` directories under each
/// file's companion namespace (`file.rs@/`). Called from plugin `activate()`.
#[allow(clippy::too_many_lines, clippy::excessive_nesting)]
pub fn register_companion_extensions(ext: &mut CompanionExtensions, state: &Arc<GitState>) {
    ext.file.scoped("git", |ext| {
        // git/
        let s = Arc::clone(state);
        ext.dir(state.vfs.dir.git.clone(), |d| {
            // on_lookup: resolve BLAME.md:{spec} and LOG.md:{spec}
            let sl = Arc::clone(&s);
            d.on_lookup(move |_ctx: &RouteCtx, req: &mut Request, name: &str| {
                let Some((handle, spec, is_blame)) = sl.resolve_sliced_view(name) else {
                    return Ok(());
                };
                let source = GitState::require_source_file(req)?;
                let read_fn = build_read_fn_file(Arc::clone(&sl.repo), source, sl.file_sliced_fetch(spec, is_blame));
                req.nodes.add(handle.lazy_node(name, read_fn));
                Ok(())
            });

            // git/BLAME.md
            let s2 = Arc::clone(&s);
            d.content(move |_ctx: &RouteCtx, req: &Request| -> Option<NamedNode> {
                s2.file_content(req, &s2.handles.blame, &s2.vfs.file.blame, |repo, rel| {
                    Ok(minijinja::context!(data => repo.blame(rel)?))
                })
            });

            // git/LOG.md
            let s2 = Arc::clone(&s);
            d.content(move |_ctx: &RouteCtx, req: &Request| -> Option<NamedNode> {
                let limits = s2.limits;
                s2.file_content(req, &s2.handles.log, &s2.vfs.file.log, move |repo, rel| {
                    Ok(minijinja::context!(data => repo.file_history(rel, limits.log)?))
                })
            });

            // git/CONTRIBUTORS.md
            let s2 = Arc::clone(&s);
            d.content(move |_ctx: &RouteCtx, req: &Request| -> Option<NamedNode> {
                let limits = s2.limits;
                s2.file_content(
                    req,
                    &s2.handles.contributors,
                    &s2.vfs.file.contributors,
                    move |repo, rel| Ok(minijinja::context!(data => repo.contributors(rel, limits.contributors)?)),
                )
            });

            // git/NOTES.md (readable + writable)
            let s2 = Arc::clone(&s);
            d.content(move |_ctx: &RouteCtx, req: &Request| -> Option<NamedNode> {
                let limits = s2.limits;
                s2.editable_file_content(
                    req,
                    &s2.handles.notes,
                    &s2.vfs.file.notes,
                    move |repo, rel| Ok(minijinja::context!(data => repo.file_notes(rel, limits.notes)?)),
                    |repo, rel, data| {
                        let message = from_utf8(data).wrap_err("note content must be valid UTF-8")?;
                        repo.set_note(rel, message)
                    },
                )
            });
        });

        // diff/
        let s = Arc::clone(state);
        ext.dir(state.vfs.dir.diff.clone(), |d| {
            // on_lookup: resolve {ref}.diff against named refs
            let s2 = Arc::clone(&s);
            d.on_lookup(move |_ctx: &RouteCtx, req: &mut Request, name: &str| {
                let Some(refspec) = name.strip_suffix(".diff") else {
                    return Ok(());
                };
                if refspec == "HEAD" || refspec.is_empty() {
                    return Ok(());
                }
                let source = GitState::require_source_file(req)?;
                let repo = Arc::clone(&s2.repo);
                let rel = repo.rel_path(&source);
                let node = Node::file()
                    .with_readable(DiffContent {
                        repo,
                        rel_path: rel,
                        target: DiffTarget::Ref(refspec.to_owned()),
                    })
                    .named(name);
                req.nodes.add(node);
                Ok(())
            });

            // diff/HEAD.diff
            let s2 = Arc::clone(&s);
            d.content(move |_ctx: &RouteCtx, req: &Request| -> Option<NamedNode> {
                let source = req.source_file()?;
                let repo = Arc::clone(&s2.repo);
                let rel = repo.rel_path(&source);
                Some(
                    Node::file()
                        .with_readable(DiffContent {
                            repo,
                            rel_path: rel,
                            target: DiffTarget::Workdir { source_file: source },
                        })
                        .named(&s2.vfs.file.head_diff),
                )
            });
        });

        // history/
        let s = Arc::clone(state);
        ext.dir(state.vfs.dir.history.clone(), |d| {
            d.handler(move |_ctx: &RouteCtx, req: &mut Request, next: &Next<'_>| {
                next.run(req)?;
                let source = GitState::require_source_file(req)?;
                let repo = Arc::clone(&s.repo);
                let rel = repo.rel_path(&source);
                let ext = source.extension().and_then(|e| e.to_str()).unwrap_or("");
                let entries = repo.file_history(&rel, s.limits.history)?;
                let rel: Arc<str> = Arc::from(rel);
                views::emit_history_nodes(
                    req,
                    &views::HistorySource {
                        repo: &repo,
                        rel: &rel,
                        ext,
                    },
                    entries,
                    None,
                    None,
                );
                Ok(())
            });
        });
    });
}

/// Register mount-wide git content into the companion mount extension point.
///
/// Contributes `git/` with STATUS.md, branches/, and tags/ under the
/// mount-wide companion namespace (`./@/git/`).
#[allow(clippy::excessive_nesting)]
pub fn register_mount_extensions(ext: &mut CompanionExtensions, state: &Arc<GitState>) {
    ext.mount.scoped("git", |ext| {
        let s = Arc::clone(state);
        ext.dir(state.vfs.dir.git.clone(), |d| {
            // git/STATUS.md — working-tree status (no cache)
            let s2 = Arc::clone(&s);
            let file_status = s.vfs.file.status.clone();
            d.content(move |_ctx: &RouteCtx, _req: &Request| {
                let (name, node) = s2
                    .handles
                    .status
                    .named_node(&file_status, StatusView {
                        repo: Arc::clone(&s2.repo),
                        limits: s2.limits,
                    })
                    .into_parts();
                Some(
                    node.with_cache_policy(CachePolicy::with_ttl(Duration::ZERO))
                        .named(name),
                )
            });

            // git/branches/
            let s2 = Arc::clone(&s);
            d.dir(s.vfs.dir.branches.clone(), |d| {
                let s3 = Arc::clone(&s2);
                d.on_readdir(move |_ctx: &RouteCtx, req: &mut Request| {
                    if let Some(nodes) = branch_segments_at_prefix(&s3.repo, "")? {
                        req.nodes.extend(nodes);
                    }
                    Ok(())
                });
                let s3 = Arc::clone(&s2);
                d.rest("prefix", |d| {
                    d.handler(move |ctx: &RouteCtx, req: &mut Request, next: &Next| {
                        branches_nested_handler(&s3, ctx, req, next)
                    });
                });
            });

            // git/tags/
            let s2 = Arc::clone(&s);
            d.dir(s.vfs.dir.tags.clone(), |d| {
                d.on_readdir(move |_ctx: &RouteCtx, req: &mut Request| {
                    req.nodes.extend(s2.repo.tags()?.into_iter().map(NamedNode::dir));
                    Ok(())
                });
            });
        });
    });
}

/// Handler for branches/{..prefix} — resolves nested branch segments or browses a branch tree.
fn branches_nested_handler(state: &GitState, ctx: &RouteCtx, req: &mut Request, next: &Next) -> Result<()> {
    next.run(req)?;
    let segs: Vec<&str> = ctx.param("prefix").unwrap_or("").split('/').collect();

    // Try as a branch namespace prefix first.
    let mut ns_prefix = segs.join("/");
    ns_prefix.push('/');
    if let Some(nodes) = branch_segments_at_prefix(&state.repo, &ns_prefix)? {
        req.nodes.extend(nodes);
        return Ok(());
    }

    // Not a namespace — find the longest branch name that is a prefix of the segments.
    let mut branches = state.repo.branches()?;
    branches.sort();
    #[allow(clippy::indexing_slicing)] // split in 1..=segs.len() — always in bounds
    for split in (1..=segs.len()).rev() {
        let candidate = segs[..split].join("/");
        if branches.binary_search(&candidate).is_err() {
            continue;
        }
        if let Some(nodes) = branch_tree_nodes(&state.repo, &candidate, &segs[split..].join("/"))? {
            req.nodes.extend(nodes);
        }
        return Ok(());
    }

    Ok(())
}

/// Decorate companion directory nodes with git-aware rename capability.
///
/// Called by [`GitProvider::accept`] after downstream dispatch completes
/// for per-file companion requests.
pub(super) fn decorate_companion_rename(req: &mut Request, repo: &Arc<Repo>, fs: &Arc<dyn Filesystem>) {
    let Some(companion) = req.companion() else {
        return;
    };
    let Some(source_file) = companion.source_file.clone() else {
        return;
    };
    let Op::Lookup { name } = req.op() else { return };
    if companion.strip_suffix(name).is_none() {
        return;
    }
    let name = name.clone();
    let Some(node) = req.nodes.find_mut(&name) else { return };
    let caps = Node::dir()
        .with_renameable(GitFileRename {
            repo: Some(Arc::clone(repo)),
            fs: Arc::clone(fs),
            source_file,
        })
        .named(&*name);
    node.merge_capabilities_from(caps);
}
