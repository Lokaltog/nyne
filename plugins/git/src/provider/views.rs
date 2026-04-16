//! Shared constants and helpers for git template views.

use std::sync::Arc;

use nyne::router::{Node, Request};
use nyne::text;

use crate::history::{HistoryEntry, HistoryVersionContent, SymbolExtractCtx};
use crate::repo::Repo;

/// Format a history entry filename: `001_2024-01-15_abc1234_commit-message.rs`.
///
/// The zero-padded sequence number keeps entries sorted chronologically in
/// directory listings. The message is slugified and truncated to 50 chars.
pub fn history_filename(index: usize, entry: &HistoryEntry, ext: &str) -> String {
    let seq = index + 1;
    let c = &entry.commit;
    let kebab = text::slugify(&c.message, 50);
    if ext.is_empty() {
        format!("{seq:03}_{}_{}_{kebab}", c.date, c.hash)
    } else {
        format!("{seq:03}_{}_{}_{kebab}.{ext}", c.date, c.hash)
    }
}

/// Blame template content.
pub const BLAME_TEMPLATE: &str = include_str!("templates/blame.md.j2");

/// Log template content.
pub const LOG_TEMPLATE: &str = include_str!("templates/log.md.j2");

/// Emit a history-version node per commit into `req.nodes`.
///
/// Shared by the file-level history handler (`register_companion_extensions`)
/// and the symbol-level `history_nodes` callback. Optionally filters to a
/// single filename (lookup path) and returns early once matched.
pub fn emit_history_nodes(
    req: &mut Request,
    repo: &Arc<Repo>,
    rel: &Arc<str>,
    ext: &str,
    entries: Vec<HistoryEntry>,
    symbol_ctx: Option<&Arc<SymbolExtractCtx>>,
    filter_name: Option<&str>,
) {
    for (i, entry) in entries.into_iter().enumerate() {
        let filename = history_filename(i, &entry, ext);
        if filter_name.is_some_and(|n| n != filename) {
            continue;
        }
        req.nodes.add(
            Node::file()
                .with_readable(HistoryVersionContent {
                    repo: Arc::clone(repo),
                    rel_path: Arc::clone(rel),
                    oid: entry.oid,
                    symbol_ctx: symbol_ctx.cloned(),
                })
                .named(filename),
        );
        if filter_name.is_some() {
            return;
        }
    }
}
