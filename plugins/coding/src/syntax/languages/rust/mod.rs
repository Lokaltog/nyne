//! Rust language decomposer.

use std::ops::Range;

use super::prelude::*;

/// File extensions handled by the Rust decomposer — SSOT for both
/// syntax and LSP language registration.
pub const EXTENSIONS: &[&str] = &["rs"];

struct RustLanguage;

impl RustLanguage {
    /// Tree-sitter node kind for attribute annotations (`#[...]`).
    const ATTRIBUTE: &str = "attribute_item";
    /// Tree-sitter node kind for single-line comments (`// ...`, `/// ...`, `//! ...`).
    const LINE_COMMENT: &str = "line_comment";
}

impl LanguageSpec for RustLanguage {
    const DOC_COMMENT_KIND: Option<&'static str> = Some(Self::LINE_COMMENT);
    const DOC_COMMENT_PREFIXES: &'static [&'static str] = &["///"];
    const DOC_COMMENT_SKIP_KINDS: &'static [&'static str] = &[Self::ATTRIBUTE];
    const EXTENSIONS: &'static [&'static str] = EXTENSIONS;
    const IMPORT_KINDS: &'static [&'static str] = &["use_declaration"];
    const NAME: &'static str = "Rust";
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

    fn extract_file_doc(root: TsNode<'_>) -> Option<String> {
        let doc_lines: Vec<_> = root
            .children()
            .take_while(|child| child.kind() == Self::LINE_COMMENT && child.text().starts_with("//!"))
            .map(|child| child.text().to_owned())
            .collect();
        if doc_lines.is_empty() {
            return None;
        }
        let joined = doc_lines.join("\n");
        Self::clean_doc_comment(&joined)
    }

    fn strip_doc_comment(raw: &str) -> String { strip_line_comment_prefixes(raw, &["///", "//!"]) }

    fn wrap_doc_comment(plain: &str, indent: &str) -> String { wrap_line_doc_comment(plain, indent, "///", "/// ") }

    fn extract_visibility(node: TsNode<'_>) -> Option<String> { extract_child_visibility(node, "visibility_modifier") }

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
