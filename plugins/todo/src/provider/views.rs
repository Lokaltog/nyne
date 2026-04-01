use std::collections::HashMap;

use serde::Serialize;

use super::entry::Entry;
use super::state::Index;

/// Overview view — all TODO entries grouped by tag and file.
#[derive(Serialize)]
pub struct OverviewView {
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
/// Convert a [`Entry`] to a serializable [`EntryView`] for template rendering.
impl From<&Entry> for EntryView {
    fn from(entry: &Entry) -> Self {
        Self {
            line: entry.line,
            tag: entry.tag.to_string(),
            text: entry.text.clone(),
        }
    }
}
/// Tag view — all entries for a specific tag.
#[derive(Serialize)]
pub struct TagView {
    tag: String,
    files: Vec<FileGroup>,
}
/// Group sorted entries by source file path.
///
/// Entries must already be sorted with file path as the primary key —
/// consecutive entries from the same file are merged into one `FileGroup`.
fn group_by_file(entries: &[&Entry]) -> Vec<FileGroup> {
    entries
        .chunk_by(|a, b| a.source_file == b.source_file)
        .filter_map(|chunk| {
            let first = chunk.first()?;
            Some(FileGroup {
                path: first.source_file.display().to_string(),
                entries: chunk.iter().map(|e| EntryView::from(*e)).collect(),
            })
        })
        .collect()
}
/// Build the overview view: all tags, grouped by file, entries sorted by
/// priority and line number.
pub fn build_overview_view(index: &Index, tag_order: &[String]) -> OverviewView {
    // Build a priority map from tag order (SSOT).
    let priority: HashMap<&str, usize> = tag_order.iter().enumerate().map(|(i, t)| (t.as_str(), i)).collect();

    // Collect all entries, flatten across tags.
    let mut all_entries: Vec<&Entry> = index.entries_by_tag.values().flat_map(|v| v.iter()).collect();

    // Sort by file path, then by tag priority, then by line number.
    all_entries.sort_by(|a, b| {
        a.source_file
            .cmp(&b.source_file)
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
pub fn build_tag_view(tag: &str, entries: &[Entry]) -> TagView {
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.source_file.cmp(&b.source_file).then_with(|| a.line.cmp(&b.line)));

    TagView {
        tag: tag.to_owned(),
        files: group_by_file(&sorted),
    }
}
