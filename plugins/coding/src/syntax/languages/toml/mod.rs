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
        let mut fragments = Vec::new();
        let mut cursor = root.raw().walk();

        // Coalesce bare top-level pairs (before any table header) into a
        // single preamble fragment. Tables are opaque sections.
        let mut preamble_start: Option<usize> = None;
        let mut preamble_end: usize = 0;

        for child in root.raw().children(&mut cursor) {
            let node = TsNode::new(child, root.source());
            match child.kind() {
                "pair" | "comment" => {
                    preamble_start.get_or_insert_with(|| child.start_byte());
                    preamble_end = child.end_byte();
                }
                "table" | "table_array_element" => {
                    fragments.push(build_table_fragment(node));
                }
                _ => {}
            }
        }

        // Insert preamble at the front if any bare pairs were found.
        if let Some(start) = preamble_start {
            let span = start..preamble_end;
            fragments.insert(0, Fragment::structural("preamble", FragmentKind::Preamble, span, None));
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

/// Build an opaque fragment for a `table` or `table_array_element` node.
///
/// Tables are not decomposed further — individual key-value pairs inside a
/// section are part of the section body, not separate symbols.
fn build_table_fragment(node: TsNode<'_>) -> Fragment {
    let name = extract_key_name(node);
    let signature = build_table_signature(node);
    let doc_range = TomlLanguage::extract_doc_range(node);
    let parent = Some(name.clone());

    let mut children = Vec::new();
    if let Some(range) = doc_range {
        children.push(Fragment::structural(
            "docstring",
            FragmentKind::Docstring,
            range,
            parent,
        ));
    }

    build_code_fragment(
        node,
        CodeFragmentSpec {
            name,
            kind: SymbolKind::Module,
            signature,
            name_byte_offset: node.name_start_byte().unwrap_or_else(|| node.start_byte()),
            visibility: None,
            children,
        },
        None,
    )
}

register_syntax!(TomlLanguage);

#[cfg(test)]
mod tests;
