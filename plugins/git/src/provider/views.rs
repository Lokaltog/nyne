//! Template view structs for git file-level content.
//!
//! These handle sliced blame/log and per-file companion directory children.
//! Symbol-scoped views (`SymbolBlameView`, `SymbolLogView`) live in nyne-coding.

use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::format;
use nyne::node::VirtualNode;
use nyne::templates::{TemplateEngine, TemplateView};
use nyne::types::SymbolLineRange;
use nyne::types::slice::SliceSpec;

use super::blame::BlameView;
use super::contributors::ContributorsView;
use super::history::HistoryEntry;
use super::log::LogView;
use super::notes::NotesView;
use super::repo::FileViewCtx;
use super::{CommitMtime, GitProvider};
use crate::names;
use crate::repo::GitRepo;

/// Default cap on history entries listed in readdir.
pub const HISTORY_LIMIT: usize = 50;

impl GitProvider {
    /// Resolve the `file.rs@/git/` companion directory — blame, log, contributors, notes.
    pub(super) fn resolve_companion_git(&self, repo: &Arc<GitRepo>, rel: String) -> Vec<VirtualNode> {
        let secs = repo.file_epoch_secs(&rel);
        let fctx = FileViewCtx::new(repo, rel);
        let h = &self.handles;
        let notes_view = NotesView(fctx.clone());
        let notes_writable = NotesView(fctx.clone());
        vec![
            h.blame
                .node(names::FILE_BLAME, BlameView(fctx.clone()))
                .with_lifecycle(CommitMtime(secs)),
            h.log
                .node(names::FILE_LOG, LogView(fctx.clone()))
                .with_lifecycle(CommitMtime(secs)),
            h.contributors
                .node(names::FILE_CONTRIBUTORS, ContributorsView(fctx))
                .with_lifecycle(CommitMtime(secs)),
            h.notes
                .node(names::FILE_NOTES, notes_view)
                .with_writable(notes_writable)
                .with_lifecycle(CommitMtime(secs)),
        ]
    }
}

/// Format a history entry filename: `001_2024-01-15_abc1234_commit-message.rs`
pub fn history_filename(index: usize, entry: &HistoryEntry, ext: &str) -> String {
    let seq = index + 1;
    let kebab = format::to_kebab(&entry.commit.message, 50);
    if ext.is_empty() {
        format!("{seq:03}_{}_{}_{kebab}", entry.commit.date, entry.commit.hash)
    } else {
        format!("{seq:03}_{}_{}_{kebab}.{ext}", entry.commit.date, entry.commit.hash)
    }
}

/// Check whether a blame hunk overlaps a 1-based inclusive line range.
pub const fn hunk_overlaps_range(hunk: &super::history::BlameHunk, range: &SymbolLineRange) -> bool {
    hunk.start_line <= range.end && hunk.end_line >= range.start
}

/// Blame content sliced by a spec (e.g., `BLAME.md:5-20` → entries 5-20).
pub(super) struct SlicedBlameView {
    pub ctx: FileViewCtx,
    pub spec: SliceSpec,
}

impl TemplateView for SlicedBlameView {
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let hunks = self.ctx.repo.blame(&self.ctx.rel_path)?;
        let sliced = self.spec.apply(&hunks);
        Ok(engine.render_bytes(template, &minijinja::context!(data => sliced)))
    }
}

/// Log content sliced by a spec (e.g., `LOG.md:-10` → last 10 entries).
pub(super) struct SlicedLogView {
    pub ctx: FileViewCtx,
    pub spec: SliceSpec,
}

impl TemplateView for SlicedLogView {
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let entries = self.ctx.repo.file_history(&self.ctx.rel_path, super::log::LOG_LIMIT)?;
        let sliced = self.spec.apply(&entries);
        Ok(engine.render_bytes(template, &minijinja::context!(data => sliced)))
    }
}

/// Blame template content (shared with nyne-coding for symbol-scoped views).
pub const BLAME_TEMPLATE: &str = include_str!("templates/blame.md.j2");

/// Log template content (shared with nyne-coding for symbol-scoped views).
pub const LOG_TEMPLATE: &str = include_str!("templates/log.md.j2");
