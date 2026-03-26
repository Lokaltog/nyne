//! Nix language decomposer.

use super::prelude::*;

/// Nix language specification for tree-sitter decomposition.
struct NixLanguage;

/// [`LanguageSpec`] implementation for Nix.
impl LanguageSpec for NixLanguage {
    /// Conflict resolution strategy for Nix symbols.
    const CONFLICT_STRATEGY: ConflictStrategy = ConflictStrategy::Numbered;
    /// Tree-sitter node kind for Nix doc comments.
    const DOC_COMMENT_KIND: Option<&'static str> = Some("comment");
    /// Comment prefix patterns for Nix doc comments.
    const DOC_COMMENT_PREFIXES: &'static [&'static str] = &["#"];
    /// File extensions for Nix.
    const EXTENSIONS: &'static [&'static str] = &["nix"];
    /// Tree-sitter node kinds for Nix import declarations.
    const IMPORT_KINDS: &'static [&'static str] = &[];
    /// Language name identifier.
    const NAME: &'static str = "Nix";
    /// Tree-sitter node kinds that support recursive decomposition in Nix.
    const RECURSABLE_KINDS: &'static [&'static str] = &[];

    /// Returns the tree-sitter grammar for Nix.
    fn grammar(_ext: &str) -> tree_sitter::Language { tree_sitter_nix::LANGUAGE.into() }

    /// Extracts Nix-specific fragments from the syntax tree.
    fn extract_custom(root: TsNode<'_>, _max_depth: usize) -> Option<Vec<Fragment>> {
        let mut fragments = Vec::new();
        collect_nix_fragments(root, &mut fragments, None);
        Some(fragments)
    }

    /// Strips doc comment markers from Nix source.
    fn strip_doc_comment(raw: &str) -> String { strip_line_comment_prefixes(raw, &["#"]) }

    /// Wraps text in Nix doc comment syntax.
    fn wrap_doc_comment(plain: &str, indent: &str) -> String { wrap_line_doc_comment(plain, indent, "#", "# ") }
}

/// Recursively collect fragments from a Nix AST.
///
/// Walks through the tree looking for \`binding\` nodes (attribute assignments)
/// and \``let_expression`\` / \``function_expression`\` at the top level. Bindings
/// whose value is an \``attrset_expression`\` are treated as sections with children.
fn collect_nix_fragments(node: TsNode<'_>, fragments: &mut Vec<Fragment>, parent_name: Option<&str>) {
    for child in node.children() {
        match child.kind() {
            "binding" => {
                fragments.push(build_binding_fragment(child, parent_name));
            }
            // Recurse into structural nodes that contain bindings.
            "binding_set"
            | "source_code"
            | "function_expression"
            | "let_expression"
            | "attrset_expression"
            | "rec_attrset_expression"
            | "with_expression" => {
                collect_nix_fragments(child, fragments, parent_name);
            }
            _ => {}
        }
    }
}

/// Extract the dotted attribute path name from a `binding` node.
fn extract_binding_name(node: TsNode<'_>) -> String {
    for child in node.children() {
        if child.kind() == "attrpath" {
            return child.text().to_owned();
        }
    }
    "unknown".to_owned()
}

/// Find the value expression (RHS) of a `binding` node.
fn binding_value_kind(node: TsNode<'_>) -> Option<&'static str> {
    let mut past_eq = false;
    for child in node.children() {
        if child.kind() == "=" {
            past_eq = true;
            continue;
        }
        if past_eq && child.kind() != ";" {
            // Use raw() for the return — tree-sitter kind strings are 'static
            // but TsNode::kind() elides the lifetime to &self.
            return Some(child.raw().kind());
        }
    }
    None
}

/// Build a fragment for a Nix \`binding\` node (\`name = value;\`).
///
/// If the value is an attribute set, recurse into it to create child fragments.
fn build_binding_fragment(node: TsNode<'_>, parent_name: Option<&str>) -> Fragment {
    let name = extract_binding_name(node);
    let signature = node.first_line().to_owned();
    let doc_range = NixLanguage::extract_doc_range(node);

    let value_kind = binding_value_kind(node);
    let is_attrset = matches!(value_kind, Some("attrset_expression" | "rec_attrset_expression"));

    let parent = Some(name.clone());

    let mut children: Vec<Fragment> = Fragment::docstring_child(doc_range, parent).into_iter().collect();
    if is_attrset {
        collect_nix_fragments(node, &mut children, Some(&name));
    }

    let kind = if is_attrset {
        SymbolKind::Module
    } else {
        SymbolKind::Variable
    };

    build_code_fragment(
        node,
        CodeFragmentSpec {
            name,
            kind,
            signature,
            name_byte_offset: node.start_byte(),
            visibility: None,
            children,
        },
        parent_name,
    )
}

register_syntax!(NixLanguage);

/// Tests for Nix decomposition.
#[cfg(test)]
mod tests;
