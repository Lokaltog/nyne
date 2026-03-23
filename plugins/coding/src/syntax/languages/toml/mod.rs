//! TOML language decomposer.

use super::prelude::*;

struct TomlLanguage;

impl LanguageSpec for TomlLanguage {
    const CONFLICT_STRATEGY: ConflictStrategy = ConflictStrategy::Numbered;
    const DOC_COMMENT_KIND: Option<&'static str> = Some("comment");
    const DOC_COMMENT_PREFIXES: &'static [&'static str] = &["#"];
    const EXTENSIONS: &'static [&'static str] = &["toml"];
    const IMPORT_KINDS: &'static [&'static str] = &[];
    const NAME: &'static str = "TOML";
    const NAMING_STRATEGY: NamingStrategy = NamingStrategy::Identity;
    const RECURSABLE_KINDS: &'static [&'static str] = &[];

    fn grammar(_ext: &str) -> tree_sitter::Language { tree_sitter_toml_ng::LANGUAGE.into() }

    fn extract_custom(root: TsNode<'_>, _max_depth: usize) -> Option<Vec<Fragment>> {
        let source = root.source_str();
        let mut fragments = Vec::new();
        let mut cursor = root.raw().walk();

        // Collect top-level pairs (before any table header) as individual fragments.
        // Then collect each table/table_array_element as a section fragment with
        // its pairs as children.
        for child in root.raw().children(&mut cursor) {
            let node = TsNode::new(child, root.source());
            match child.kind() {
                "table" | "table_array_element" => {
                    fragments.push(build_table_fragment(node, source));
                }
                "pair" => {
                    fragments.push(build_pair_fragment(node, source, None));
                }
                _ => {}
            }
        }
        Some(fragments)
    }

    fn strip_doc_comment(raw: &str) -> String { strip_line_comment_prefixes(raw, &["#"]) }

    fn wrap_doc_comment(plain: &str, indent: &str) -> String { wrap_line_doc_comment(plain, indent, "#", "# ") }
}

/// Extract the key name from a TOML node's first key child.
///
/// Works for `table`, `table_array_element`, and `pair` nodes — all use
/// `bare_key`, `dotted_key`, or `quoted_key` as the name-bearing child.
fn extract_key_name(node: TsNode<'_>) -> String {
    let mut cursor = node.raw().walk();
    for child in node.raw().children(&mut cursor) {
        match child.kind() {
            "bare_key" | "dotted_key" | "quoted_key" => {
                return child.utf8_text(node.source()).unwrap_or("unknown").to_owned();
            }
            _ => {}
        }
    }
    "unknown".to_owned()
}

/// Build a signature for a table node (e.g. `[package]` or `[[bin]]`).
fn build_table_signature(node: TsNode<'_>) -> String { node.first_line().to_owned() }

/// Build a fragment for a `table` or `table_array_element` node, with pairs as
/// children.
fn build_table_fragment(node: TsNode<'_>, source: &str) -> Fragment {
    let name = extract_key_name(node);
    let signature = build_table_signature(node);
    let doc_range = TomlLanguage::extract_doc_range(node);

    let mut children = Vec::new();
    let mut cursor = node.raw().walk();
    for child in node.raw().children(&mut cursor) {
        if child.kind() == "pair" {
            let pair_node = TsNode::new(child, node.source());
            children.push(build_pair_fragment(pair_node, source, Some(&name)));
        }
    }

    let node_range = node.byte_range();
    let full_span = TomlLanguage::full_symbol_range(&node_range, doc_range.as_ref(), None);

    build_code_fragment(
        node,
        CodeFragmentSpec {
            name,
            kind: SymbolKind::Module,
            signature,
            name_byte_offset: node.name_start_byte().unwrap_or_else(|| node.start_byte()),
            visibility: None,
            doc_comment_range: doc_range,
            decorator_range: None,
            full_span,
            children,
        },
        None,
    )
}

/// Build a fragment for a key-value `pair` node.
fn build_pair_fragment(node: TsNode<'_>, _source: &str, parent_name: Option<&str>) -> Fragment {
    let name = extract_key_name(node);
    let signature = node.first_line().to_owned();
    let doc_range = TomlLanguage::extract_doc_range(node);

    let node_range = node.byte_range();
    let full_span = TomlLanguage::full_symbol_range(&node_range, doc_range.as_ref(), None);

    build_code_fragment(
        node,
        CodeFragmentSpec {
            name,
            kind: SymbolKind::Variable,
            signature,
            name_byte_offset: node.start_byte(),
            visibility: None,
            doc_comment_range: doc_range,
            decorator_range: None,
            full_span,
            children: Vec::new(),
        },
        parent_name,
    )
}

register_syntax!(TomlLanguage);

#[cfg(test)]
mod tests;
