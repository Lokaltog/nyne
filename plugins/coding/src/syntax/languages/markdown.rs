//! Markdown language decomposer.

use std::ops::Range;

use super::prelude::*;

struct MarkdownLanguage;

impl LanguageSpec for MarkdownLanguage {
    const CONFLICT_STRATEGY: ConflictStrategy = ConflictStrategy::Numbered;
    const EXTENSIONS: &'static [&'static str] = &["md", "mdx"];
    const IMPORT_KINDS: &'static [&'static str] = &[];
    const NAME: &'static str = "Markdown";
    const NAMING_STRATEGY: NamingStrategy = NamingStrategy::Slugified { indexed: true };
    const RECURSABLE_KINDS: &'static [&'static str] = &[];

    fn grammar(_ext: &str) -> tree_sitter::Language { tree_sitter_md::LANGUAGE.into() }

    fn extract_custom(root: TsNode<'_>, _max_depth: usize) -> Option<Vec<Fragment>> {
        let source = root.source_str();
        let headings = collect_headings(root, root.source());
        let code_blocks = collect_code_blocks(root, source);
        Some(build_section_fragments(&headings, &code_blocks, source))
    }
}

/// A heading extracted from the parse tree.
struct Heading {
    level: u8,
    name: String,
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
fn collect_headings(root: TsNode<'_>, source: &[u8]) -> Vec<Heading> {
    let mut headings = Vec::new();
    collect_headings_recursive(root, source, &mut headings);
    headings
}

fn collect_headings_recursive(node: TsNode<'_>, source: &[u8], headings: &mut Vec<Heading>) {
    if node.kind() == "atx_heading" {
        headings.push(parse_atx_heading(node, source));
    }
    for child in node.children() {
        collect_headings_recursive(child, source, headings);
    }
}

fn parse_atx_heading(node: TsNode<'_>, source: &[u8]) -> Heading {
    let mut level = 0u8;
    let mut name = None;

    for child in node.children() {
        if child.kind().starts_with("atx_h") && child.kind().ends_with("_marker") {
            level = child.text().len().try_into().unwrap_or(1);
        } else if child.kind() == "inline" {
            name = Some(child.raw().utf8_text(source).unwrap_or("").trim().to_owned());
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
    collect_code_blocks_recursive(root, source, &mut blocks);
    blocks
}

fn collect_code_blocks_recursive(node: TsNode<'_>, source: &str, blocks: &mut Vec<CodeBlock>) {
    if node.kind() == "fenced_code_block" {
        blocks.push(parse_fenced_code_block(node, source));
    }
    for child in node.children() {
        collect_code_blocks_recursive(child, source, blocks);
    }
}

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
                content_range = Some(child.start_byte()..child.raw().end_byte());
            }
            _ => {}
        }
    }

    let block_range = node.start_byte()..node.raw().end_byte();

    // If there's no content node, the block is empty — use an empty range
    // at the position after the opening fence line.
    let content_range = content_range.unwrap_or_else(|| {
        // Find the end of the opening fence line to place the empty content range.
        let fence_end = source
            .get(node.start_byte()..)
            .and_then(|s| s.find('\n'))
            .map_or_else(|| node.raw().end_byte(), |pos| node.start_byte() + pos + 1);
        fence_end..fence_end
    });

    CodeBlock {
        lang,
        content_range,
        block_range,
    }
}

/// Build hierarchically nested section fragments from headings and code blocks.
fn build_section_fragments(headings: &[Heading], code_blocks: &[CodeBlock], source: &str) -> Vec<Fragment> {
    build_sections_at_level(headings, code_blocks, source, source.len(), 0)
}

fn build_sections_at_level(
    headings: &[Heading],
    code_blocks: &[CodeBlock],
    source: &str,
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

        let child_headings = headings.get(children_start..children_end).unwrap_or(&[]);
        let children = build_sections_at_level(child_headings, code_blocks, source, end_byte, 0);

        let byte_range = heading.start_byte..end_byte;

        // Collect code blocks that belong to this section (within its byte range
        // but not inside any child section).
        let section_code_blocks = build_code_block_fragments(code_blocks, &byte_range, &children, source);

        let mut all_children = children;
        all_children.extend(section_code_blocks);

        let full_span = byte_range.clone();
        fragments.push(Fragment::new(
            source,
            heading.name.clone(),
            FragmentKind::Section { level: heading.level },
            byte_range,
            full_span,
            None,
            FragmentMetadata::Document {
                index: index_offset + fragments.len(),
            },
            heading.start_byte,
            all_children,
            None,
        ));

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
    source: &str,
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
        .map(|(idx, cb)| {
            Fragment::new(
                source,
                cb.lang.clone().unwrap_or_default(),
                FragmentKind::CodeBlock { lang: cb.lang.clone() },
                cb.content_range.clone(),
                cb.block_range.clone(),
                None,
                FragmentMetadata::CodeBlock { index: idx },
                cb.block_range.start,
                Vec::new(),
                None,
            )
        })
        .collect()
}

register_syntax!(MarkdownLanguage);
