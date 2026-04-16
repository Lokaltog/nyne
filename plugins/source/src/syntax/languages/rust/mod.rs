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

impl LanguageSpec for RustLanguage {
    /// AST node kind for doc comments.
    const DOC_COMMENT_KIND: Option<&'static str> = Some(Self::LINE_COMMENT);
    /// Prefixes that identify doc comments. Includes `//!` so file-level
    /// module docs strip cleanly through the default `strip_doc_comment`.
    const DOC_COMMENT_PREFIXES: &'static [&'static str] = &["///", "//!"];
    const DOC_COMMENT_SKIP_KINDS: &'static [&'static str] = &[Self::ATTRIBUTE];
    const DOC_COMMENT_WRITE: Option<(&'static str, &'static str)> = Some(("///", "/// "));
    /// File extensions for Rust.
    const EXTENSIONS: &'static [&'static str] = EXTENSIONS;
    const FILE_DOC_COMMENT_WRITE: Option<(&'static str, &'static str)> = Some(("//!", "//! "));
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

    fn extract_name(node: TsNode<'_>, kind: SymbolKind) -> String {
        if kind == SymbolKind::Impl {
            return impl_block_name(node);
        }
        node.field_text("name").unwrap_or("anonymous").to_owned()
    }

    fn extract_file_doc_range(root: TsNode<'_>) -> Option<Range<usize>> {
        extract_leading_file_doc_range(root, Self::LINE_COMMENT, &["//!"])
    }

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

/// Flatten a type to a filesystem-safe name by stripping generics and path separators.
///
/// Replaces `::` with `_` and removes `<`, `>`, `,`, and spaces so that
/// `std::collections::HashMap<K, V>` becomes `std_collections_HashMapKV`.
/// Used by [`impl_block_name`] to derive fragment names for impl blocks.
fn flatten_type_text(raw: &str) -> String { raw.replace("::", "_").replace(['<', '>', ',', ' '], "") }

register_syntax!(RustLanguage);

#[cfg(test)]
mod tests;
