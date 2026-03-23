//! Nix language decomposer.

use super::prelude::*;

struct NixLanguage;

impl LanguageSpec for NixLanguage {
    const CONFLICT_STRATEGY: ConflictStrategy = ConflictStrategy::Numbered;
    const DOC_COMMENT_KIND: Option<&'static str> = Some("comment");
    const DOC_COMMENT_PREFIXES: &'static [&'static str] = &["#"];
    const EXTENSIONS: &'static [&'static str] = &["nix"];
    const IMPORT_KINDS: &'static [&'static str] = &[];
    const NAME: &'static str = "Nix";
    const NAMING_STRATEGY: NamingStrategy = NamingStrategy::Identity;
    const RECURSABLE_KINDS: &'static [&'static str] = &[];

    fn grammar(_ext: &str) -> tree_sitter::Language { tree_sitter_nix::LANGUAGE.into() }

    fn extract_custom(root: TsNode<'_>, _max_depth: usize) -> Option<Vec<Fragment>> {
        let source = root.source_str();
        let mut fragments = Vec::new();
        collect_nix_fragments(root, source, &mut fragments, None);
        Some(fragments)
    }

    fn strip_doc_comment(raw: &str) -> String { strip_line_comment_prefixes(raw, &["#"]) }

    fn wrap_doc_comment(plain: &str, indent: &str) -> String { wrap_line_doc_comment(plain, indent, "#", "# ") }
}

/// Recursively collect fragments from a Nix AST.
///
/// Walks through the tree looking for `binding` nodes (attribute assignments)
/// and `let_expression` / `function_expression` at the top level. Bindings
/// whose value is an `attrset_expression` are treated as sections with children.
fn collect_nix_fragments(node: TsNode<'_>, source: &str, fragments: &mut Vec<Fragment>, parent_name: Option<&str>) {
    let mut cursor = node.raw().walk();
    for child in node.raw().children(&mut cursor) {
        let child_node = TsNode::new(child, node.source());
        match child.kind() {
            "binding" => {
                fragments.push(build_binding_fragment(child_node, source, parent_name));
            }
            // Recurse into structural nodes that contain bindings.
            "binding_set"
            | "source_code"
            | "function_expression"
            | "let_expression"
            | "attrset_expression"
            | "rec_attrset_expression"
            | "with_expression" => {
                collect_nix_fragments(child_node, source, fragments, parent_name);
            }
            _ => {}
        }
    }
}

/// Extract the dotted attribute path name from a `binding` node.
fn extract_binding_name(node: TsNode<'_>) -> String {
    let mut cursor = node.raw().walk();
    for child in node.raw().children(&mut cursor) {
        if child.kind() == "attrpath" {
            return child.utf8_text(node.source()).unwrap_or("unknown").to_owned();
        }
    }
    "unknown".to_owned()
}

/// Find the value expression (RHS) of a `binding` node.
fn binding_value_kind(node: TsNode<'_>) -> Option<&str> {
    let mut cursor = node.raw().walk();
    let mut past_eq = false;
    for child in node.raw().children(&mut cursor) {
        if child.kind() == "=" {
            past_eq = true;
            continue;
        }
        if past_eq && child.kind() != ";" {
            return Some(child.kind());
        }
    }
    None
}

/// Build a fragment for a Nix `binding` node (`name = value;`).
///
/// If the value is an attribute set, recurse into it to create child fragments.
fn build_binding_fragment(node: TsNode<'_>, source: &str, parent_name: Option<&str>) -> Fragment {
    let name = extract_binding_name(node);
    let signature = node.first_line().to_owned();
    let doc_range = NixLanguage::extract_doc_range(node);

    let value_kind = binding_value_kind(node);
    let is_attrset = matches!(value_kind, Some("attrset_expression" | "rec_attrset_expression"));

    let mut children = Vec::new();
    if is_attrset {
        collect_nix_fragments(node, source, &mut children, Some(&name));
    }

    let kind = if is_attrset {
        SymbolKind::Module
    } else {
        SymbolKind::Variable
    };

    let node_range = node.byte_range();
    let full_span = NixLanguage::full_symbol_range(&node_range, doc_range.as_ref(), None);

    build_code_fragment(
        node,
        CodeFragmentSpec {
            name,
            kind,
            signature,
            name_byte_offset: node.start_byte(),
            visibility: None,
            doc_comment_range: doc_range,
            decorator_range: None,
            full_span,
            children,
        },
        parent_name,
    )
}

register_syntax!(NixLanguage);

#[cfg(test)]
mod tests;
