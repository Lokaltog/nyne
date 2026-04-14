//! Markdown language decomposer.

use std::ops::Range;

use super::prelude::*;

/// Markdown language specification for tree-sitter decomposition.
struct MarkdownLanguage;

/// [`LanguageSpec`] implementation for Markdown.
impl LanguageSpec for MarkdownLanguage {
    const CONFLICT_STRATEGY: ConflictStrategy = ConflictStrategy::Numbered;
    /// File extensions for Markdown.
    const EXTENSIONS: &'static [&'static str] = &["md", "mdx"];
    /// Tree-sitter node kinds for Markdown import declarations.
    const IMPORT_KINDS: &'static [&'static str] = &[];
    /// Language name identifier.
    const NAME: &'static str = "Markdown";
    /// Naming strategy for Markdown symbol deduplication.
    const NAMING_STRATEGY: NamingStrategy = NamingStrategy::Slugified { indexed: true };
    const RECURSABLE_KINDS: &'static [&'static str] = &[];

    fn grammar(_ext: &str) -> tree_sitter::Language { tree_sitter_md::LANGUAGE.into() }

    /// Extracts Markdown-specific fragments (sections, code blocks, preamble).
    fn extract_custom(root: TsNode<'_>, _max_depth: usize) -> Option<Vec<Fragment>> {
        let source = root.source_str();
        let headings = collect_headings(root);
        let code_blocks = collect_code_blocks(root, source);

        let mut fragments = Vec::new();

        // Content before the first heading (frontmatter, intro text) → preamble.
        let first_heading_byte = headings.first().map_or(source.len(), |h| h.start_byte);
        if !source[..first_heading_byte].trim().is_empty() {
            fragments.push(Fragment {
                name: "preamble".to_owned(),
                kind: FragmentKind::Preamble,
                byte_range: 0..first_heading_byte,
                signature: None,
                visibility: None,
                metadata: Some(FragmentMetadata::Document { index: 0 }),
                name_byte_offset: 0,
                children: Vec::new(),
                parent_name: None,
                fs_name: None,
            });
        }

        fragments.extend(build_section_fragments(&headings, &code_blocks, source.len()));
        Some(fragments)
    }
}

/// A heading extracted from the parse tree.
///
/// Intermediate representation used between tree-sitter parsing and
/// hierarchical section construction. The `level` determines nesting
/// depth, `name` becomes the fragment name, and `start_byte` marks
/// where this section begins in the source.
struct Heading {
    /// ATX heading level (1-6).
    level: u8,
    /// Heading text content (inline text after `#` markers).
    name: String,
    /// Byte offset of the heading node in the source.
    start_byte: usize,
}

/// A fenced code block extracted from the parse tree.
struct CodeBlock {
    /// Language tag from the info string (e.g. "rust", "sh"). `None` for unlabeled blocks.
    lang: Option<String>,
    /// Byte range of the code content (excluding fences).
    content_range: Range<usize>,
    /// Byte range of the entire fenced code block (including fences).
    block_range: Range<usize>,
}

/// Collect all ATX headings from the markdown parse tree.
fn collect_headings(root: TsNode<'_>) -> Vec<Heading> {
    let mut headings = Vec::new();
    collect_descendants(root, "atx_heading", &parse_atx_heading, &mut headings);
    headings
}

/// Parse an ATX heading node into a Heading struct.
fn parse_atx_heading(node: TsNode<'_>) -> Heading {
    let mut level = 0u8;
    let mut name = None;

    for child in node.children() {
        // Matches tree-sitter-markdown marker nodes: `atx_h1_marker` through
        // `atx_h6_marker`. This is coupled to the grammar's node-kind naming
        // convention, which has been stable since the grammar's 0.1 release.
        // A lookup table or regex would add indirection without meaningful
        // benefit — if the grammar ever renames these nodes, the decomposer
        // would need broader changes regardless.
        if child.kind().starts_with("atx_h") && child.kind().ends_with("_marker") {
            level = child.text().len().try_into().unwrap_or(1);
        } else if child.kind() == "inline" {
            name = Some(child.text().trim().to_owned());
        }
    }

    Heading {
        level,
        name: name.unwrap_or_default(),
        start_byte: node.start_byte(),
    }
}

/// Collect all fenced code blocks from the markdown parse tree.
fn collect_code_blocks(root: TsNode<'_>, source: &str) -> Vec<CodeBlock> {
    let mut blocks = Vec::new();
    collect_descendants(
        root,
        "fenced_code_block",
        &|n| parse_fenced_code_block(n, source),
        &mut blocks,
    );
    blocks
}

/// Parse a fenced code block node into a `CodeBlock` struct.
fn parse_fenced_code_block(node: TsNode<'_>, source: &str) -> CodeBlock {
    let mut lang = None;
    let mut content_range = None;

    for child in node.children() {
        match child.kind() {
            "info_string" => {
                let info = child.text().trim();
                if !info.is_empty() {
                    // Extract just the language token (first word of info string).
                    let language = info.split_whitespace().next().unwrap_or(info);
                    lang = Some(language.to_owned());
                }
            }
            "code_fence_content" => {
                content_range = Some(child.start_byte()..child.end_byte());
            }
            _ => {}
        }
    }

    let block_range = node.start_byte()..node.end_byte();

    // If there's no content node, the block is empty — use an empty range
    // at the position after the opening fence line.
    let content_range = content_range.unwrap_or_else(|| {
        // Find the end of the opening fence line to place the empty content range.
        let fence_end = source
            .get(node.start_byte()..)
            .and_then(|s| s.find('\n'))
            .map_or_else(|| node.end_byte(), |pos| node.start_byte() + pos + 1);
        fence_end..fence_end
    });

    CodeBlock {
        lang,
        content_range,
        block_range,
    }
}

/// Build hierarchically nested section fragments from headings and code blocks.
fn build_section_fragments(headings: &[Heading], code_blocks: &[CodeBlock], source_len: usize) -> Vec<Fragment> {
    build_sections_at_level(headings, code_blocks, source_len, 0)
}

/// Recursively build nested section fragments from a heading level.
///
/// Finds the minimum heading level in the slice, then groups headings at
/// that level into sibling sections. Headings between two same-level
/// siblings become children of the preceding section (recursion handles
/// their nesting). Each section's byte range extends from its heading to
/// the start of the next same-or-higher-level heading (or `section_end_byte`).
///
/// Code blocks are assigned to the innermost section that contains them,
/// excluding those already inside a child section.
fn build_sections_at_level(
    headings: &[Heading],
    code_blocks: &[CodeBlock],
    section_end_byte: usize,
    index_offset: usize,
) -> Vec<Fragment> {
    if headings.is_empty() {
        return Vec::new();
    }

    let target_level = headings.iter().map(|h| h.level).min().unwrap_or(1);
    let mut fragments = Vec::new();
    let mut i = 0;

    while i < headings.len() {
        let Some(current) = headings.get(i) else {
            break;
        };
        if current.level != target_level {
            i += 1;
            continue;
        }

        let heading = current;

        let end_byte = headings
            .get(i + 1..)
            .into_iter()
            .flatten()
            .find(|h| h.level <= target_level)
            .map_or(section_end_byte, |h| h.start_byte);

        let children_start = i + 1;
        let children_end = headings
            .get(children_start..)
            .into_iter()
            .flatten()
            .position(|h| h.level <= target_level)
            .map_or(headings.len(), |pos| children_start + pos);

        let children = build_sections_at_level(
            headings.get(children_start..children_end).unwrap_or(&[]),
            code_blocks,
            end_byte,
            0,
        );

        let byte_range = heading.start_byte..end_byte;

        // Collect code blocks that belong to this section (within its byte range
        // but not inside any child section).
        let section_code_blocks = build_code_block_fragments(code_blocks, &byte_range, &children);

        let mut all_children = children;
        all_children.extend(section_code_blocks);

        fragments.push(Fragment {
            name: heading.name.clone(),
            kind: FragmentKind::Section { level: heading.level },
            byte_range,
            signature: None,
            visibility: None,
            metadata: Some(FragmentMetadata::Document {
                index: index_offset + fragments.len(),
            }),
            name_byte_offset: heading.start_byte,
            children: all_children,
            parent_name: None,
            fs_name: None,
        });

        i = children_end;
    }

    fragments
}

/// Build code block fragments for a section, filtering to blocks that are directly
/// owned by this section (inside its byte range but not inside any child section).
fn build_code_block_fragments(
    code_blocks: &[CodeBlock],
    section_range: &Range<usize>,
    child_sections: &[Fragment],
) -> Vec<Fragment> {
    code_blocks
        .iter()
        .filter(|cb| {
            // Block must be within this section's byte range.
            cb.block_range.start >= section_range.start && cb.block_range.end <= section_range.end
        })
        .filter(|cb| {
            // Block must not be inside any child section.
            !child_sections.iter().any(|child| {
                cb.block_range.start >= child.byte_range.start && cb.block_range.end <= child.byte_range.end
            })
        })
        .enumerate()
        .map(|(idx, cb)| Fragment {
            name: cb.lang.clone().unwrap_or_default(),
            kind: FragmentKind::CodeBlock { lang: cb.lang.clone() },
            byte_range: cb.content_range.clone(),
            signature: None,
            visibility: None,
            metadata: Some(FragmentMetadata::CodeBlock { index: idx }),
            name_byte_offset: cb.block_range.start,
            children: Vec::new(),
            parent_name: None,
            fs_name: None,
        })
        .collect()
}

register_syntax!(MarkdownLanguage);

/// Tests for Markdown decomposition.
#[cfg(test)]
mod tests;
