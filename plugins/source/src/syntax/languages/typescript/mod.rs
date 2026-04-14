//! TypeScript/JavaScript language decomposer.

use std::ops::Range;

use super::prelude::*;

/// File extensions handled by the TypeScript decomposer — SSOT for both
/// syntax and LSP language registration.
pub const EXTENSIONS: &[&str] = &["ts", "tsx"];

/// TypeScript/JavaScript language specification for tree-sitter decomposition.
struct TypeScriptLanguage;

/// Constants for TypeScript tree-sitter node kinds.
impl TypeScriptLanguage {
    /// Tree-sitter node kind for `export` wrapper statements.
    const EXPORT_STATEMENT: &str = "export_statement";
}

/// [`LanguageSpec`] implementation for TypeScript/JavaScript.
impl LanguageSpec for TypeScriptLanguage {
    /// File extensions for TypeScript.
    const EXTENSIONS: &'static [&'static str] = EXTENSIONS;
    /// AST node kinds that represent imports.
    const IMPORT_KINDS: &'static [&'static str] = &["import_statement"];
    /// Language name identifier.
    const NAME: &'static str = "TypeScript";
    const RECURSABLE_KINDS: &'static [&'static str] =
        &["class_declaration", "abstract_class_declaration", "internal_module"];

    symbol_map! {
        "function_declaration"       => Function,
        "method_definition"          => Function,
        "class_declaration"          => Class,
        "abstract_class_declaration" => Class,
        "interface_declaration"      => Interface,
        "type_alias_declaration"     => TypeAlias,
        "enum_declaration"           => Enum,
        "lexical_declaration"        => Variable,
        "internal_module"            => Module,
    }

    /// Returns the tree-sitter grammar for the given file extension.
    fn grammar(ext: &str) -> tree_sitter::Language {
        match ext {
            "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
            _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        }
    }

    /// Unwraps export statements to expose the inner symbol.
    fn unwrap_wrapper(node: TsNode<'_>) -> Option<(TsNode<'_>, WrapperInfo)> {
        match node.kind() {
            Self::EXPORT_STATEMENT => {
                let inner = node
                    .children()
                    .into_iter()
                    .find(|c| Self::map_symbol_kind(c.kind()).is_some())?;
                Some((inner, WrapperInfo {
                    visibility: Some("export".to_owned()),
                    decorator_range: None,
                }))
            }
            "expression_statement" => {
                let inner = node
                    .children()
                    .into_iter()
                    .find(|c| Self::map_symbol_kind(c.kind()).is_some())?;
                Some((inner, WrapperInfo::default()))
            }
            _ => None,
        }
    }

    /// Builds a display signature for the symbol.
    fn build_signature(node: TsNode<'_>, kind: SymbolKind) -> String {
        match kind {
            SymbolKind::Variable => node.first_line().trim_end_matches(';').trim().to_owned(),
            _ => node.text_up_to('{'),
        }
    }

    /// Extracts the symbol name from the AST node.
    fn extract_name(node: TsNode<'_>, kind: SymbolKind) -> String {
        if kind == SymbolKind::Variable {
            return extract_variable_name(node);
        }
        node.field_text("name").unwrap_or("anonymous").to_owned()
    }

    fn extract_doc_range(node: TsNode<'_>) -> Option<Range<usize>> {
        extract_preceding_doc_range(unwrap_export(node), "comment", &["/**", "///", "//"], &[])
    }

    /// Strips doc comment prefix from a line.
    fn strip_doc_comment(raw: &str) -> String {
        if raw.starts_with("/**") {
            strip_jsdoc(raw)
        } else {
            strip_line_comment_prefixes(raw, &["///", "//"])
        }
    }

    /// Wraps text in `JSDoc` comment syntax.
    fn wrap_doc_comment(plain: &str, indent: &str) -> String {
        let mut result = String::from("/**\n");
        for line in plain.lines() {
            result.push_str(indent);
            result.push_str(" * ");
            result.push_str(line);
            result.push('\n');
        }
        result.push_str(indent);
        result.push_str(" */");
        result
    }

    /// Extracts the decorator range preceding a node.
    fn extract_decorator_range(node: TsNode<'_>) -> Option<Range<usize>> {
        extract_preceding_decorator_range(unwrap_export(node), "decorator")
    }
}

/// If `node` is inside an `export_statement`, return the export; otherwise
/// return the node itself. Used to look for preceding doc/decorator siblings
/// on the correct outer node.
fn unwrap_export(node: TsNode<'_>) -> TsNode<'_> {
    let Some(parent) = node.parent() else { return node };
    if parent.kind() == TypeScriptLanguage::EXPORT_STATEMENT {
        parent
    } else {
        node
    }
}

/// Extract the variable name from a `lexical_declaration` via `variable_declarator`.
///
/// TypeScript `const`/`let`/`var` declarations have the AST shape
/// `lexical_declaration > variable_declarator(name: identifier)`. This
/// navigates that structure to find the identifier. Falls back to
/// `"anonymous"` for destructuring patterns or other edge cases where
/// the `name` field is absent.
fn extract_variable_name(node: TsNode<'_>) -> String {
    for child in node.children() {
        if child.kind() == "variable_declarator"
            && let Some(name) = child.field_text("name")
        {
            return name.to_owned();
        }
    }
    "anonymous".to_owned()
}

/// Strip `JSDoc` `/** ... */` markers and leading ` * ` from each line.
///
/// Handles both single-line (`/** text */`) and multi-line `JSDoc` blocks.
/// Each intermediate line's leading ` * ` or bare `*` prefix is removed.
/// The result is trimmed of surrounding whitespace to produce clean
/// content for `docstring.txt` in the VFS.
fn strip_jsdoc(raw: &str) -> String {
    let inner = raw
        .strip_prefix("/**")
        .and_then(|s| s.strip_suffix("*/"))
        .unwrap_or(raw);
    let mut out = String::with_capacity(inner.len());
    for (i, line) in inner.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let trimmed = line.trim();
        out.push_str(
            trimmed
                .strip_prefix("* ")
                .or_else(|| trimmed.strip_prefix('*'))
                .unwrap_or(trimmed),
        );
    }
    let trimmed = out.trim();
    if trimmed.len() == out.len() {
        out
    } else {
        trimmed.to_owned()
    }
}

register_syntax!(TypeScriptLanguage);

/// Tests for TypeScript decomposition.
#[cfg(test)]
mod tests;
