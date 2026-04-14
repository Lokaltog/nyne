//! Fennel language decomposer.

use super::prelude::*;

/// Fennel language specification for tree-sitter decomposition.
struct FennelLanguage;

/// [`LanguageSpec`] implementation for Fennel.
impl LanguageSpec for FennelLanguage {
    const CONFLICT_STRATEGY: ConflictStrategy = ConflictStrategy::Numbered;
    const DOC_COMMENT_KIND: Option<&'static str> = Some("comment");
    /// Comment prefix patterns for Fennel doc comments.
    const DOC_COMMENT_PREFIXES: &'static [&'static str] = &[";"];
    const EXTENSIONS: &'static [&'static str] = &["fnl"];
    const IMPORT_KINDS: &'static [&'static str] = &[];
    const NAME: &'static str = "Fennel";
    const RECURSABLE_KINDS: &'static [&'static str] = &[];
    const SPLICE_MODE: SpliceMode = SpliceMode::Byte;

    /// Returns the tree-sitter grammar for Fennel.
    fn grammar(_ext: &str) -> tree_sitter::Language { tree_sitter_fennel::LANGUAGE.into() }

    /// Extracts Fennel-specific fragments from the syntax tree.
    fn extract_custom(root: TsNode<'_>, _max_depth: usize) -> Option<Vec<Fragment>> {
        let mut fragments = Vec::new();
        collect_fennel_fragments(root, &mut fragments, None);
        Some(fragments)
    }

    /// Strips doc comment markers from Fennel source.
    fn strip_doc_comment(raw: &str) -> String { strip_line_comment_prefixes(raw, &[";;;", ";;", ";"]) }

    fn wrap_doc_comment(plain: &str, indent: &str) -> String { wrap_line_doc_comment(plain, indent, ";;", ";; ") }
}

/// Recursively collect fragments from a Fennel AST.
///
/// Top-level forms (`fn_form`, `lambda_form`, `local_form`, `var_form`,
/// `macro_form`) are extracted as symbols.
fn collect_fennel_fragments(node: TsNode<'_>, fragments: &mut Vec<Fragment>, parent_name: Option<&str>) {
    for child in node.children() {
        let (kind, name) = match child.kind() {
            "fn_form" | "lambda_form" => (
                SymbolKind::Function,
                fennel_form_name(child, &["symbol", "multi_symbol"], "anonymous"),
            ),
            "local_form" | "var_form" if !is_require_binding(child) => {
                let Some(name) = extract_binding_name(child) else {
                    continue;
                };
                (SymbolKind::Variable, name)
            }
            "macro_form" => (SymbolKind::Macro, fennel_form_name(child, &["symbol"], "unknown")),
            "program" => {
                collect_fennel_fragments(child, fragments, parent_name);
                continue;
            }
            _ => continue,
        };
        let doc_range = FennelLanguage::extract_doc_range(child);
        fragments.push(build_simple_fragment(child, name, kind, doc_range, parent_name));
    }
}

/// Find the first post-keyword named child matching any of `name_kinds`.
///
/// Fennel forms start with a keyword symbol (`fn`, `lambda`, `macro`, …).
/// This helper skips the keyword, then scans for the first child with a
/// matching kind and returns its text. Returns `default` if no match is
/// found, or if `sequence_arguments` appears before a match (indicating an
/// anonymous form).
fn fennel_form_name(node: TsNode<'_>, name_kinds: &[&str], default: &str) -> String {
    let mut past_keyword = false;
    for child in node.children() {
        if child.kind() == "symbol" && !past_keyword {
            past_keyword = true;
            continue;
        }
        if !past_keyword {
            continue;
        }
        if name_kinds.contains(&child.kind()) {
            return child.text().to_owned();
        }
        if child.kind() == "sequence_arguments" {
            return default.to_owned();
        }
    }
    default.to_owned()
}

/// Extract the binding name from a \``local_form`\` or \``var_form`\` node.
///
/// The name is in the \``binding_pair`\` → \``symbol_binding`\` child.
fn extract_binding_name(node: TsNode<'_>) -> Option<String> {
    let binding_pair = node.children().into_iter().find(|c| c.kind() == "binding_pair")?;
    let symbol = binding_pair
        .children()
        .into_iter()
        .find(|c| c.kind() == "symbol_binding")?;
    Some(symbol.text().to_owned())
}

/// Check if a \``local_form`\` or \``var_form`\` is a require binding.
///
/// Matches \`(local name (require :module))\` — the binding pair's value
/// is a list whose first symbol is \`require\`. These are excluded from
/// fragment extraction because they are effectively import statements,
/// not user-defined symbols.
fn is_require_binding(node: TsNode<'_>) -> bool {
    let Some(binding_pair) = node.children().into_iter().find(|c| c.kind() == "binding_pair") else {
        return false;
    };
    binding_pair
        .children()
        .into_iter()
        .filter(|c| c.kind() != "symbol_binding")
        .any(|value| value.text().starts_with("(require "))
}

register_syntax!(FennelLanguage);

/// Tests for Fennel decomposition.
#[cfg(test)]
mod tests;
