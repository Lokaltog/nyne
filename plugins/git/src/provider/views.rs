//! Shared constants and helpers for git template views.

use nyne::text;

use crate::history::HistoryEntry;

/// Format a history entry filename: `001_2024-01-15_abc1234_commit-message.rs`.
///
/// The zero-padded sequence number keeps entries sorted chronologically in
/// directory listings. The message is slugified and truncated to 50 chars.
pub fn history_filename(index: usize, entry: &HistoryEntry, ext: &str) -> String {
    let seq = index + 1;
    let kebab = text::slugify(&entry.message, 50);
    if ext.is_empty() {
        format!("{seq:03}_{}_{}_{kebab}", entry.date, entry.hash)
    } else {
        format!("{seq:03}_{}_{}_{kebab}.{ext}", entry.date, entry.hash)
    }
}

/// Blame template content.
pub const BLAME_TEMPLATE: &str = include_str!("templates/blame.md.j2");

/// Log template content.
pub const LOG_TEMPLATE: &str = include_str!("templates/log.md.j2");
