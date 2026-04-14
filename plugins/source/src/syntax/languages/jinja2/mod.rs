//! Jinja2 template content extraction and structural symbol decomposition.
//!
//! Walks the Jinja2 tree-sitter AST to extract:
//! - **Content regions**: byte ranges of plain-text `content` nodes (the inner
//!   language text between template directives).
//! - **Structural symbols**: `block` → Module, `macro` → Function, `set` →
//!   Variable.
//!
//! Block pairing (matching `{% block %}` with `{% endblock %}`) is done via a
//! linear scan with a stack, since tree-sitter-jinja represents open and close
//! tags as separate sibling `control` nodes rather than nesting them.
//!
//! ## Single-parse API
//!
//! [`extract_template`] is the primary entry point — it parses once and returns
//! both content regions and structural symbols. Callers should never need to
//! parse the same source twice.

use std::ops::Range;

use crate::syntax::fragment::{Fragment, FragmentKind, SymbolKind};
use crate::syntax::parser::{TreeSitterParser, TsNode, find_first_descendant};

/// Result of extracting template structure from a Jinja2 source file.
///
/// Produced by [`extract_template`] — a single parse yields both content
/// regions (for the inner-language decomposer) and structural symbols
/// (for the Jinja2-layer fragments).
#[derive(Debug, Clone)]
pub struct TemplateExtraction {
    /// Byte ranges of inner-language content in the original source.
    ///
    /// These are the `content` nodes from the Jinja2 AST — the text between
    /// template directives. Suitable for passing to [`SpanMap::build`].
    pub regions: Vec<Range<usize>>,

    /// Jinja2 structural symbols (blocks, macros, variables).
    pub symbols: Vec<Jinja2Symbol>,
}

/// A Jinja2 structural symbol extracted from the template AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jinja2Symbol {
    pub name: String,
    pub kind: FragmentKind,
    /// Signature line (e.g. `{% block title %}`, `{% macro render(items) %}`).
    pub signature: String,
    /// Byte offset of the name token in the original source.
    pub name_byte_offset: usize,
    /// Full span from the opening tag start to the closing tag end.
    pub full_span: Range<usize>,
}

/// Create a tree-sitter parser for Jinja2.
pub fn jinja2_parser() -> TreeSitterParser { TreeSitterParser::new(&tree_sitter_jinja::language()) }

/// Extract content regions and structural symbols from a Jinja2 template.
///
/// This is the single entry point — parses once and returns everything the
/// injection decomposer needs. Callers must not parse again.
pub fn extract_template(source: &str) -> TemplateExtraction {
    let parser = jinja2_parser();
    let Some(tree) = parser.parse(source) else {
        return TemplateExtraction {
            regions: Vec::new(),
            symbols: Vec::new(),
        };
    };
    let root = tree.root_node();

    let mut regions = Vec::new();
    let mut symbols = Vec::new();
    let mut block_stack: Vec<PendingBlock> = Vec::new();
    let mut preamble_start: Option<usize> = None;
    let mut preamble_end: usize = 0;

    // The tree-sitter-jinja AST is flat: all nodes (content, control,
    // render_expression) are direct children of `source`.
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "content" => {
                let range = child.byte_range();
                if !range.is_empty() {
                    regions.push(range);
                }
            }
            "control" if is_preamble_directive(child, source) => {
                if preamble_start.is_none() {
                    preamble_start = Some(child.start_byte());
                }
                preamble_end = child.end_byte();
            }
            "control" => {
                handle_control_node(child, source, &mut symbols, &mut block_stack);
            }
            _ => {}
        }
    }

    // Insert preamble (extends/import) as the first symbol.
    if let Some(start) = preamble_start {
        symbols.insert(0, Jinja2Symbol {
            name: "preamble".to_owned(),
            kind: FragmentKind::Preamble,
            signature: String::new(),
            name_byte_offset: start,
            full_span: start..preamble_end,
        });
    }

    TemplateExtraction { regions, symbols }
}

/// A directive that has been opened but not yet closed.
///
/// All paired directives (block, macro, for, if) are pushed here. When the
/// corresponding end tag is found, the entry is popped. Only block and macro
/// entries produce symbols; for/if entries are discarded on close.
struct PendingBlock {
    name: String,
    /// `Some` for directives that should become symbols (block, macro).
    /// `None` for directives we track only for correct stack pairing (for, if).
    emittable: Option<EmittableSymbol>,
    open_start: usize,
}

/// Data for a pending directive that will become a [`Jinja2Symbol`] on close.
struct EmittableSymbol {
    kind: FragmentKind,
    signature: String,
    name_byte_offset: usize,
}

/// Process a `control` node to extract structural symbols.
///
/// End tags in tree-sitter-jinja appear as `(control (statement))` — a
/// statement node with no named children. We detect them by checking the
/// source text for `end` keywords.
///
/// All paired directives (block, macro, for, if) are pushed onto the stack
/// so that end tags pop the correct entry. Only block and macro produce
/// symbols; for/if are silently discarded on close.
///
/// Other empty-statement control nodes (`{% else %}`, `{% elif %}`) have
/// no named children but don't contain `"end"`, so they fall through
/// harmlessly.
fn handle_control_node(
    node: tree_sitter::Node<'_>,
    source: &str,
    symbols: &mut Vec<Jinja2Symbol>,
    block_stack: &mut Vec<PendingBlock>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() != "statement" {
            continue;
        }
        let first_named = {
            let mut c = child.walk();
            child.named_children(&mut c).next()
        };

        if let Some(stmt) = first_named {
            match stmt.kind() {
                // Paired emittable: block → Module, macro → Function.
                "block_statement" => {
                    push_paired(
                        node,
                        stmt,
                        source,
                        FragmentKind::Symbol(SymbolKind::Module),
                        block_stack,
                    );
                }
                "macro_statement" => {
                    push_paired(
                        node,
                        stmt,
                        source,
                        FragmentKind::Symbol(SymbolKind::Function),
                        block_stack,
                    );
                }
                // Paired non-emittable: pushed for correct stack pairing only.
                "for_statement" | "if_expression" => {
                    block_stack.push(PendingBlock {
                        name: String::new(),
                        emittable: None,
                        open_start: node.start_byte(),
                    });
                }
                // Unpaired: set is a single directive, no close tag.
                "set_statement" => {
                    let (name, name_offset) = extract_identifier(stmt, source);
                    let signature = control_signature(node, source);
                    symbols.push(Jinja2Symbol {
                        name,
                        kind: FragmentKind::Symbol(SymbolKind::Variable),
                        signature,
                        name_byte_offset: name_offset,
                        full_span: node.byte_range(),
                    });
                }
                _ => {}
            }
        } else if is_end_tag_text(node, source) {
            // No named children + contains "end" → close tag.
            if let Some(pending) = block_stack.pop()
                && let Some(emit) = pending.emittable
            {
                symbols.push(Jinja2Symbol {
                    name: pending.name,
                    kind: emit.kind,
                    signature: emit.signature,
                    name_byte_offset: emit.name_byte_offset,
                    full_span: pending.open_start..node.end_byte(),
                });
            }
        }
    }
}

/// Push a paired emittable directive (block/macro) onto the stack.
fn push_paired(
    control_node: tree_sitter::Node<'_>,
    stmt_child: tree_sitter::Node<'_>,
    source: &str,
    kind: FragmentKind,
    block_stack: &mut Vec<PendingBlock>,
) {
    let (name, name_offset) = extract_identifier(stmt_child, source);
    let signature = control_signature(control_node, source);
    block_stack.push(PendingBlock {
        name,
        emittable: Some(EmittableSymbol {
            kind,
            signature,
            name_byte_offset: name_offset,
        }),
        open_start: control_node.start_byte(),
    });
}

/// Extract the first line of a control node's text as a trimmed signature.
fn control_signature(node: tree_sitter::Node<'_>, source: &str) -> String {
    source
        .get(node.byte_range())
        .unwrap_or_default()
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// Check if a control node is an end tag by looking at its source text.
///
/// End tags are `{% endblock %}`, `{% endmacro %}`, `{% endfor %}`, etc.
/// Safe from false positives like `{% set endgame = 1 %}` because those
/// have named children and never reach this check (the `None` guard in
/// `handle_control_node` ensures we only get here for empty statements).
fn is_end_tag_text(node: tree_sitter::Node<'_>, source: &str) -> bool {
    let text = source.get(node.byte_range()).unwrap_or_default().trim();
    text.starts_with("{%") && text.contains("end")
}

/// Check if a control node is an extends or import directive.
///
/// These are preamble directives that should be collected into a single
/// preamble fragment rather than decomposed individually. The whitespace
/// control characters (`-`, `+`) used in Jinja2 trim modes
/// (e.g. `{%- extends ... -%}`) are stripped before keyword matching.
fn is_preamble_directive(node: tree_sitter::Node<'_>, source: &str) -> bool {
    let text = source.get(node.byte_range()).unwrap_or_default().trim();
    if !text.starts_with("{%") {
        return false;
    }
    let inner = text
        .trim_start_matches("{%")
        .trim_start_matches(['-', '+'])
        .trim_start();
    inner.starts_with("extends") || inner.starts_with("import") || inner.starts_with("from")
}

/// Extract the identifier name and byte offset from a statement node.
///
/// The grammar nests identifiers differently per statement type:
/// - `block_statement` → direct `identifier` child
/// - `macro_statement` → `function_call > identifier`
/// - `set_statement` → first `expression > ... > identifier`
///
/// We do a depth-first search for the first `identifier` node.
fn extract_identifier(node: tree_sitter::Node<'_>, source: &str) -> (String, usize) {
    let wrapped = TsNode::new(node, source.as_bytes());
    if let Some(ident) = find_first_descendant(wrapped, "identifier") {
        return (ident.text().to_owned(), ident.start_byte());
    }
    // Fallback: use the statement kind as name.
    let name = node.kind().replace("_statement", "");
    (name, node.start_byte())
}

/// Convert extracted [`Jinja2Symbol`]s into [`Fragment`]s.
///
/// Takes ownership to avoid unnecessary cloning — the caller (injection
/// decomposer) doesn't need the symbols after conversion.
pub fn symbols_to_fragments(symbols: Vec<Jinja2Symbol>) -> Vec<Fragment> {
    symbols
        .into_iter()
        .map(|sym| Fragment {
            name: sym.name,
            kind: sym.kind,
            byte_range: sym.full_span,
            signature: (!sym.signature.is_empty()).then_some(sym.signature),
            visibility: None,
            metadata: None,
            name_byte_offset: sym.name_byte_offset,
            children: Vec::new(),
            parent_name: None,
            fs_name: None,
        })
        .collect()
}

/// Tests for Jinja2 template decomposition.
#[cfg(test)]
mod tests;
