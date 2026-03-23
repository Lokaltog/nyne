//! Fennel language decomposer.

use super::prelude::*;

struct FennelLanguage;

impl LanguageSpec for FennelLanguage {
    const CONFLICT_STRATEGY: ConflictStrategy = ConflictStrategy::Numbered;
    const DOC_COMMENT_KIND: Option<&'static str> = Some("comment");
    const DOC_COMMENT_PREFIXES: &'static [&'static str] = &[";"];
    const EXTENSIONS: &'static [&'static str] = &["fnl"];
    const IMPORT_KINDS: &'static [&'static str] = &[];
    const NAME: &'static str = "Fennel";
    const NAMING_STRATEGY: NamingStrategy = NamingStrategy::Identity;
    const RECURSABLE_KINDS: &'static [&'static str] = &[];
    const SPLICE_MODE: SpliceMode = SpliceMode::Byte;

    fn grammar(_ext: &str) -> tree_sitter::Language { tree_sitter_fennel::LANGUAGE.into() }

    fn extract_custom(root: TsNode<'_>, _max_depth: usize) -> Option<Vec<Fragment>> {
        let source = root.source_str();
        let mut fragments = Vec::new();
        collect_fennel_fragments(root, source, &mut fragments, None);
        Some(fragments)
    }

    fn strip_doc_comment(raw: &str) -> String { strip_line_comment_prefixes(raw, &[";;;", ";;", ";"]) }

    fn wrap_doc_comment(plain: &str, indent: &str) -> String { wrap_line_doc_comment(plain, indent, ";;", ";; ") }
}

/// Recursively collect fragments from a Fennel AST.
///
/// Top-level forms (`fn_form`, `lambda_form`, `local_form`, `var_form`,
/// `macro_form`) are extracted as symbols.
fn collect_fennel_fragments(node: TsNode<'_>, source: &str, fragments: &mut Vec<Fragment>, parent_name: Option<&str>) {
    let mut cursor = node.raw().walk();
    for child in node.raw().children(&mut cursor) {
        let child_node = TsNode::new(child, node.source());
        match child.kind() {
            "fn_form" | "lambda_form" => {
                fragments.push(build_fn_fragment(child_node, source, parent_name));
            }
            "local_form" | "var_form" if !is_require_binding(child_node) =>
                if let Some(frag) = build_binding_fragment(child_node, source, parent_name) {
                    fragments.push(frag);
                },
            "macro_form" => {
                fragments.push(build_macro_fragment(child_node, source, parent_name));
            }
            // Recurse into structural containers.
            "program" => {
                collect_fennel_fragments(child_node, source, fragments, parent_name);
            }
            _ => {}
        }
    }
}

/// Extract the function name from a `fn_form` or `lambda_form` node.
///
/// The name is the first `symbol` or `multi_symbol` child after the keyword.
fn extract_fn_name(node: TsNode<'_>) -> String {
    let mut cursor = node.raw().walk();
    let mut past_keyword = false;
    for child in node.raw().children(&mut cursor) {
        if child.kind() == "symbol" && !past_keyword {
            // First symbol is the keyword ("fn", "lambda") — skip it.
            past_keyword = true;
            continue;
        }
        if past_keyword && (child.kind() == "symbol" || child.kind() == "multi_symbol") {
            return child.utf8_text(node.source()).unwrap_or("anonymous").to_owned();
        }
        // If we hit arguments before finding a name, it's anonymous.
        if child.kind() == "sequence_arguments" {
            return "anonymous".to_owned();
        }
    }
    "anonymous".to_owned()
}

/// Build a fragment for a `fn_form` or `lambda_form`.
fn build_fn_fragment(node: TsNode<'_>, _source: &str, parent_name: Option<&str>) -> Fragment {
    build_fennel_fragment(node, extract_fn_name(node), SymbolKind::Function, parent_name)
}

/// Extract the binding name from a `local_form` or `var_form` node.
///
/// The name is in the `binding_pair` → `symbol_binding` child.
fn extract_binding_name(node: TsNode<'_>) -> Option<String> {
    let mut cursor = node.raw().walk();
    let binding_pair = node.raw().children(&mut cursor).find(|c| c.kind() == "binding_pair")?;
    let mut inner_cursor = binding_pair.walk();
    let symbol = binding_pair
        .children(&mut inner_cursor)
        .find(|c| c.kind() == "symbol_binding")?;
    Some(symbol.utf8_text(node.source()).unwrap_or("unknown").to_owned())
}

/// Check if a `local_form` or `var_form` is a require binding.
///
/// Matches `(local name (require :module))` — the binding pair's value
/// is a list whose first symbol is `require`.
fn is_require_binding(node: TsNode<'_>) -> bool {
    let mut cursor = node.raw().walk();
    let Some(binding_pair) = node.raw().children(&mut cursor).find(|c| c.kind() == "binding_pair") else {
        return false;
    };
    let mut inner = binding_pair.walk();
    binding_pair
        .children(&mut inner)
        .filter(|c| c.kind() != "symbol_binding")
        .any(|value| {
            let text = value.utf8_text(node.source()).unwrap_or("");
            text.starts_with("(require ")
        })
}

/// Build a fragment for a `local_form` or `var_form`.
fn build_binding_fragment(node: TsNode<'_>, _source: &str, parent_name: Option<&str>) -> Option<Fragment> {
    Some(build_fennel_fragment(
        node,
        extract_binding_name(node)?,
        SymbolKind::Variable,
        parent_name,
    ))
}

/// Extract the macro name from a `macro_form` node.
///
/// The name is the second `symbol` child (first is the "macro" keyword).
fn extract_macro_name(node: TsNode<'_>) -> String {
    let mut cursor = node.raw().walk();
    let mut past_keyword = false;
    for child in node.raw().children(&mut cursor) {
        if child.kind() == "symbol" {
            if !past_keyword {
                past_keyword = true;
                continue;
            }
            return child.utf8_text(node.source()).unwrap_or("unknown").to_owned();
        }
    }
    "unknown".to_owned()
}

/// Build a fragment for a `macro_form`.
fn build_macro_fragment(node: TsNode<'_>, _source: &str, parent_name: Option<&str>) -> Fragment {
    build_fennel_fragment(node, extract_macro_name(node), SymbolKind::Macro, parent_name)
}

/// Shared builder for Fennel fragment types.
///
/// All Fennel symbols share the same structure: extract doc range via the
/// `LanguageSpec` default, compute the full span, and delegate to
/// [`build_code_fragment`].
fn build_fennel_fragment(node: TsNode<'_>, name: String, kind: SymbolKind, parent_name: Option<&str>) -> Fragment {
    let signature = node.first_line().to_owned();
    let doc_range = FennelLanguage::extract_doc_range(node);
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
            kind,
            signature,
            name_byte_offset: node.start_byte(),
            visibility: None,
            children,
        },
        parent_name,
    )
}

register_syntax!(FennelLanguage);

#[cfg(test)]
mod tests;
