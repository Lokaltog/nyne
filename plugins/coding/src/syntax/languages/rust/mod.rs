//! Rust language decomposer.

use std::ops::Range;

use super::prelude::*;

/// File extensions handled by the Rust decomposer — SSOT for both
/// syntax and LSP language registration.
pub const EXTENSIONS: &[&str] = &["rs"];

/// Rust language specification for tree-sitter decomposition.
struct RustLanguage;

/// Constants for Rust tree-sitter node kinds.
impl RustLanguage {
    /// Tree-sitter node kind for attribute annotations (`#[...]`).
    const ATTRIBUTE: &str = "attribute_item";
    /// Tree-sitter node kind for single-line comments (`// ...`, `/// ...`, `//! ...`).
    const LINE_COMMENT: &str = "line_comment";
}

/// [`LanguageSpec`] implementation for Rust.
impl LanguageSpec for RustLanguage {
    /// AST node kind for doc comments.
    const DOC_COMMENT_KIND: Option<&'static str> = Some(Self::LINE_COMMENT);
    /// Prefixes that identify doc comments.
    const DOC_COMMENT_PREFIXES: &'static [&'static str] = &["///"];
    /// AST node kinds to skip when scanning for doc comments.
    const DOC_COMMENT_SKIP_KINDS: &'static [&'static str] = &[Self::ATTRIBUTE];
    /// File extensions for Rust.
    const EXTENSIONS: &'static [&'static str] = EXTENSIONS;
    /// AST node kinds that represent imports.
    const IMPORT_KINDS: &'static [&'static str] = &["use_declaration"];
    /// Language name identifier.
    const NAME: &'static str = "Rust";
    /// AST node kinds that can contain nested symbols.
    const RECURSABLE_KINDS: &'static [&'static str] = &["impl_item", "trait_item", "mod_item"];

    symbol_map! {
        "function_item"     => Function,
        "struct_item"       => Struct,
        "enum_item"         => Enum,
        "trait_item"        => Trait,
        "const_item"        => Const,
        "static_item"       => Static,
        "type_item"         => TypeAlias,
        "impl_item"         => Impl,
        "macro_definition"  => Macro,
        "mod_item"          => Module,
    }

    /// Returns the tree-sitter grammar.
    fn grammar(_ext: &str) -> tree_sitter::Language { tree_sitter_rust::LANGUAGE.into() }

    /// Builds a display signature for the symbol.
    fn build_signature(node: TsNode<'_>, kind: SymbolKind) -> String {
        match kind {
            SymbolKind::Function | SymbolKind::Impl => node.text_up_to('{'),
            SymbolKind::Struct => node.type_signature("struct", Self::extract_visibility(node).as_deref()),
            SymbolKind::Enum => node.type_signature("enum", Self::extract_visibility(node).as_deref()),
            SymbolKind::Trait => node.type_signature("trait", Self::extract_visibility(node).as_deref()),
            SymbolKind::Const | SymbolKind::Static => node.text_up_to('='),
            SymbolKind::TypeAlias => node.text().trim_end_matches(';').trim().to_owned(),
            SymbolKind::Macro => {
                format!("macro_rules! {}", node.field_text("name").unwrap_or("?"))
            }
            _ => node.first_line().to_owned(),
        }
    }

    /// Extracts the symbol name from the AST node.
    fn extract_name(node: TsNode<'_>, kind: SymbolKind) -> String {
        if kind == SymbolKind::Impl {
            return impl_block_name(node);
        }
        node.field_text("name").unwrap_or("anonymous").to_owned()
    }

    /// Extracts the module-level docstring range.
    fn extract_file_doc_range(root: TsNode<'_>) -> Option<Range<usize>> {
        let doc_nodes: Vec<_> = root
            .children()
            .take_while(|child| child.kind() == Self::LINE_COMMENT && child.text().starts_with("//!"))
            .collect();
        let first = doc_nodes.first()?;
        let last = doc_nodes.last()?;
        let start = first.start_byte();
        let mut end = last.raw().end_byte();
        // Trim trailing newlines — same convention as merge_preceding_sibling_ranges.
        let source = root.source();
        while end > start && source.get(end - 1) == Some(&b'\n') {
            end -= 1;
        }
        Some(start..end)
    }

    /// Strips doc comment prefix from a line.
    fn strip_doc_comment(raw: &str) -> String { strip_line_comment_prefixes(raw, &["///", "//!"]) }

    /// Wraps text in doc comment syntax.
    fn wrap_doc_comment(plain: &str, indent: &str) -> String { wrap_line_doc_comment(plain, indent, "///", "/// ") }

    /// Wraps text in file-level doc comment syntax.
    fn wrap_file_doc_comment(plain: &str, indent: &str) -> String {
        wrap_line_doc_comment(plain, indent, "//!", "//! ")
    }

    /// Extracts the visibility modifier from a node.
    fn extract_visibility(node: TsNode<'_>) -> Option<String> { extract_child_visibility(node, "visibility_modifier") }

    /// Extracts the decorator/attribute range preceding a node.
    fn extract_decorator_range(node: TsNode<'_>) -> Option<Range<usize>> {
        extract_preceding_decorator_range(node, Self::ATTRIBUTE)
    }
}

/// Derive a name for an `impl` block. Trait impls become
/// `Trait_for_Type`, inherent impls become just `Type`.
fn impl_block_name(node: TsNode<'_>) -> String {
    let type_name = node
        .field("type")
        .map_or_else(|| "Unknown".to_owned(), |n| flatten_type_text(n.text()));
    match node.field("trait").map(|n| flatten_type_text(n.text())) {
        Some(t) => format!("{t}_for_{type_name}"),
        None => type_name,
    }
}

/// Flatten a type to a simple text representation for use in names.
fn flatten_type_text(raw: &str) -> String { raw.replace("::", "_").replace(['<', '>', ',', ' '], "") }

register_syntax!(RustLanguage);

/// Tests for Rust decomposition.
#[cfg(test)]
mod tests;
