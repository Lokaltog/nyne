//! TODO scanner — Aho-Corasick automaton for tag detection.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use std::ops::Range;
use std::str::from_utf8;
use std::sync::Arc;

use aho_corasick::AhoCorasick;
use nyne::types::real_fs::RealFs;
use nyne::types::vfs_path::VfsPath;
use nyne_source::{Decomposer, SyntaxRegistry};

use super::entry::TodoEntry;

/// Pre-built scanner for TODO/FIXME/etc. tags.
///
/// The automaton is built once from the configured tags and reused for all
/// file scans. Tag order in the config determines priority (index 0 = highest).
pub(super) struct TodoScanner {
    automaton: AhoCorasick,
    /// Canonical tag strings, indexed to match automaton pattern IDs.
    tags: Vec<Arc<str>>,
}

/// Methods for [`TodoScanner`].
impl TodoScanner {
    /// Build a scanner from the configured tags.
    ///
    /// Tags are matched case-insensitively; the canonical case from the
    /// config is used for categorization and display.
    #[allow(clippy::expect_used)] // simple string patterns, build cannot fail
    /// Create a new scanner with the given tag patterns.
    pub fn new(tags: &[String]) -> Self {
        let automaton = AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build(tags)
            .expect("aho-corasick build should not fail for simple string patterns");
        Self {
            automaton,
            tags: tags.iter().map(|t| Arc::from(t.as_str())).collect(),
        }
    }

    /// Check if file content contains any configured tag.
    pub fn has_tags(&self, content: &str) -> bool { self.automaton.is_match(content) }

    /// Scan a single file for TODO entries.
    ///
    /// Flow:
    /// 1. Read file content via `real_fs`
    /// 2. Fast aho-corasick pre-filter — skip files with no tag matches
    /// 3. Find all tag match positions and their line numbers
    /// 4. For each match, extract the comment block from the source text
    /// 5. Strip comment prefixes using the decomposer
    pub fn scan_file(
        &self,
        source_file: &VfsPath,
        real_fs: &dyn RealFs,
        decomposer: &dyn Decomposer,
    ) -> Vec<TodoEntry> {
        let Ok(bytes) = real_fs.read(source_file) else {
            return Vec::new();
        };
        let Ok(source) = from_utf8(&bytes) else {
            return Vec::new();
        };

        if !self.has_tags(source) {
            return Vec::new();
        }

        let line_starts = build_line_starts(source);

        // Use tree-sitter to identify comment regions structurally.
        // Falls back to prefix heuristic when the parse returns no tree.
        let comment_ranges = decomposer
            .decompose(source, 0)
            .1
            .map(|tree| collect_comment_ranges(&tree))
            .unwrap_or_default();

        // Find all tag matches with their positions.
        let matches: Vec<TagMatch> = self.find_tag_matches(source, &line_starts, &comment_ranges);
        if matches.is_empty() {
            return Vec::new();
        }

        let mut seen = HashSet::new();

        matches
            .iter()
            .filter(|m| seen.insert((m.byte_offset, m.pattern_id)))
            .filter_map(|m| {
                let block = find_comment_block(source, &line_starts, m.byte_offset)?;
                let raw_comment = source.get(m.byte_offset..block.end)?;
                let tag = self.tags.get(m.pattern_id)?;
                let stripped = strip_tag_prefix(raw_comment, tag)?;
                let text = decomposer.strip_doc_comment(&stripped).trim().to_owned();
                (!text.is_empty()).then(|| TodoEntry {
                    source_file: source_file.clone(),
                    line: byte_to_line(&line_starts, m.byte_offset) + 1,
                    tag: Arc::clone(tag),
                    text,
                })
            })
            .collect()
    }

    /// Scan all files, returning entries grouped by tag (preserving tag priority order).
    pub fn scan_all(
        &self,
        files: &[VfsPath],
        real_fs: &dyn RealFs,
        syntax: &SyntaxRegistry,
    ) -> BTreeMap<String, Vec<TodoEntry>> {
        let mut by_tag: BTreeMap<String, Vec<TodoEntry>> = BTreeMap::new();

        // Pre-initialize with configured tag order.
        for tag in &self.tags {
            by_tag.insert(tag.to_string(), Vec::new());
        }

        let entries = files.iter().filter_map(|file| {
            let ext = file.extension()?;
            let decomposer = syntax.get(ext)?;
            Some(self.scan_file(file, real_fs, decomposer.as_ref()))
        });
        for entry in entries.flatten() {
            let Some(bucket) = by_tag.get_mut(&*entry.tag) else {
                continue;
            };
            bucket.push(entry);
        }

        by_tag
    }

    /// Find all tag matches, filtering to those inside comments.
    ///
    /// Uses tree-sitter comment ranges when available for accurate detection
    /// across all languages. Falls back to a prefix heuristic when ranges are
    /// absent (unsupported language or parse failure).
    fn find_tag_matches(&self, source: &str, line_starts: &[usize], comment_ranges: &[Range<usize>]) -> Vec<TagMatch> {
        self.automaton
            .find_iter(source)
            .filter_map(|mat| {
                let byte_offset = mat.start();
                let in_comment = if comment_ranges.is_empty() {
                    prefix_heuristic(source, line_starts, byte_offset)
                } else {
                    ranges_contain(comment_ranges, byte_offset)
                };
                in_comment.then(|| TagMatch {
                    byte_offset,
                    pattern_id: mat.pattern().as_usize(),
                })
            })
            .collect()
    }
}

/// A single Aho-Corasick match in the source text.
///
/// Raw match before validation — may be inside a string literal or
/// non-comment context. [`TodoScanner::find_tag_matches`] filters
/// these to only those inside actual comment blocks.
struct TagMatch {
    /// Byte offset of the match start in the source.
    byte_offset: usize,
    /// Index into `tags` — identifies which tag matched.
    pattern_id: usize,
}

/// Build a table mapping line index to byte offset of line start.
fn build_line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

/// Convert a byte offset to a 0-based line number.
fn byte_to_line(line_starts: &[usize], byte_offset: usize) -> usize {
    match line_starts.binary_search(&byte_offset) {
        Ok(line) => line,
        Err(line) => line.saturating_sub(1),
    }
}
/// Collect sorted, non-overlapping byte ranges of all comment nodes in the tree.
///
/// Walks the tree-sitter syntax tree and collects the byte range of every node
/// whose kind contains `"comment"` (covers `comment`, `line_comment`,
/// `block_comment`, `comment_block`, etc. across grammars). Ranges are returned
/// sorted by start offset for binary-search lookup.
fn collect_comment_ranges(tree: &tree_sitter::Tree) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = tree.walk();
    collect_comments_recursive(&mut cursor, &mut ranges);
    ranges
}

/// Depth-first traversal collecting comment node byte ranges.
fn collect_comments_recursive(cursor: &mut tree_sitter::TreeCursor, out: &mut Vec<Range<usize>>) {
    loop {
        let node = cursor.node();
        if node.kind().contains("comment") {
            out.push(node.start_byte()..node.end_byte());
            // Comment nodes have no interesting children — skip descent.
        } else if cursor.goto_first_child() {
            collect_comments_recursive(cursor, out);
            cursor.goto_parent();
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// Check whether `offset` falls inside any of the sorted, non-overlapping `ranges`.
fn ranges_contain(ranges: &[Range<usize>], offset: usize) -> bool {
    ranges
        .binary_search_by(|r| {
            if offset < r.start {
                Ordering::Greater
            } else if offset >= r.end {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        })
        .is_ok()
}

/// Fallback prefix heuristic for comment detection when tree-sitter is unavailable.
///
/// Checks whether a byte offset falls on a line that looks like a comment using
/// common prefix patterns (`//`, `#`, `/*`, `* `).
fn prefix_heuristic(source: &str, line_starts: &[usize], byte_offset: usize) -> bool {
    let line = byte_to_line(line_starts, byte_offset);
    let Some(&line_start) = line_starts.get(line) else {
        return false;
    };
    let line_end = source
        .get(line_start..)
        .and_then(|s| s.find('\n'))
        .map_or(source.len(), |pos| line_start + pos);
    let Some(trimmed) = source.get(line_start..line_end).map(str::trim_start) else {
        return false;
    };
    trimmed.starts_with("//")
        || trimmed.starts_with('#')
        || trimmed.starts_with("* ")
        || trimmed.starts_with("/*")
        || source
            .get(line_start..byte_offset)
            .is_some_and(|before| before.contains("//") || before.contains('#') || before.contains("/*"))
}

/// Find the contiguous comment block containing `byte_offset`.
///
/// Walks backward and forward from the match line to find all contiguous
/// comment lines (lines starting with a comment prefix).
fn find_comment_block(source: &str, line_starts: &[usize], byte_offset: usize) -> Option<Range<usize>> {
    let match_line = byte_to_line(line_starts, byte_offset);

    // Determine the comment prefix from the match line.
    let &line_start = line_starts.get(match_line)?;
    let line_end = line_starts.get(match_line + 1).copied().unwrap_or(source.len());
    let line_text = source.get(line_start..line_end)?.trim_start();

    // Block comment — treat the whole block as one unit.
    if line_text.starts_with("/*") || line_text.starts_with('*') {
        return find_block_comment_range(source, byte_offset);
    }

    let prefix = if line_text.starts_with("///") || line_text.starts_with("//!") || line_text.starts_with("//") {
        "//"
    } else if line_text.starts_with('#') {
        "#"
    } else {
        // Inline comment: check if there's a comment delimiter before the match.
        let before = source.get(line_start..byte_offset)?;
        if let Some(offset) = before.rfind("//") {
            // Inline line comment — return just this line's comment portion.
            let comment_start = line_start + offset;
            return Some(comment_start..line_end);
        }
        if before.contains('#') {
            "#"
        } else {
            return None;
        }
    };

    // Walk backward to find the start of the contiguous comment block.
    let mut block_start_line = match_line;
    while block_start_line > 0 {
        let &prev_start = line_starts.get(block_start_line - 1)?;
        let &prev_end = line_starts.get(block_start_line)?;
        let prev_text = source.get(prev_start..prev_end)?.trim_start();
        if prev_text.starts_with(prefix) {
            block_start_line -= 1;
        } else {
            break;
        }
    }

    // Walk forward to find the end of the contiguous comment block.
    let mut block_end_line = match_line;
    while block_end_line + 1 < line_starts.len() {
        let &next_start = line_starts.get(block_end_line + 1)?;
        let next_end = line_starts.get(block_end_line + 2).copied().unwrap_or(source.len());
        if next_start >= source.len() {
            break;
        }
        let next_text = source.get(next_start..next_end)?.trim_start();
        if next_text.starts_with(prefix) && !next_text.trim().is_empty() {
            block_end_line += 1;
        } else {
            break;
        }
    }

    let &start = line_starts.get(block_start_line)?;
    let end = line_starts.get(block_end_line + 1).copied().unwrap_or(source.len());

    Some(start..end)
}

/// Find the range of a `/* ... */` block comment containing the byte offset.
fn find_block_comment_range(source: &str, byte_offset: usize) -> Option<Range<usize>> {
    // Search backward for `/*`.
    let before = &source[..byte_offset];
    let start = before.rfind("/*")?;

    // Search forward for `*/`.
    let after = &source[byte_offset..];
    let end_relative = after.find("*/")?;
    let end = byte_offset + end_relative + 2;

    Some(start..end)
}

/// Strip the tag prefix from comment text, requiring a colon separator.
///
/// Delegates to [`nyne_source::parse_tag_suffix`] for the colon requirement.
fn strip_tag_prefix(text: &str, tag: &str) -> Option<String> {
    nyne_source::parse_tag_suffix(&text[tag.len()..]).map(str::to_owned)
}

/// Unit tests.
#[cfg(test)]
mod tests;
