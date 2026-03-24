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
    const EXTENSIONS: &'static [&'static str] = EXTENSIONS;
    const IMPORT_KINDS: &'static [&'static str] = &["import_statement"];
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

    fn grammar(ext: &str) -> tree_sitter::Language {
        match ext {
            "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
            _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        }
    }

    fn unwrap_wrapper(node: TsNode<'_>) -> Option<(TsNode<'_>, WrapperInfo)> {
        match node.kind() {
            Self::EXPORT_STATEMENT => {
                let inner = node.children().find(|c| Self::map_symbol_kind(c.kind()).is_some())?;
                Some((inner, WrapperInfo {
                    visibility: Some("export".to_owned()),
                    decorator_range: None,
                }))
            }
            "expression_statement" => {
                let inner = node.children().find(|c| Self::map_symbol_kind(c.kind()).is_some())?;
                Some((inner, WrapperInfo::default()))
            }
            _ => None,
        }
    }

    fn build_signature(node: TsNode<'_>, kind: SymbolKind) -> String {
        match kind {
            SymbolKind::Variable => node.first_line().trim_end_matches(';').trim().to_owned(),
            _ => node.text_up_to('{'),
        }
    }

    fn extract_name(node: TsNode<'_>, kind: SymbolKind) -> String {
        if kind == SymbolKind::Variable {
            return extract_variable_name(node);
        }
        node.field_text("name").unwrap_or("anonymous").to_owned()
    }

    fn extract_doc_range(node: TsNode<'_>) -> Option<Range<usize>> {
        extract_preceding_doc_range(unwrap_export(node)?, "comment", &["/**", "///", "//"], &[])
    }

    fn strip_doc_comment(raw: &str) -> String {
        if raw.starts_with("/**") {
            strip_jsdoc(raw)
        } else {
            strip_line_comment_prefixes(raw, &["///", "//"])
        }
    }

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

    fn extract_decorator_range(node: TsNode<'_>) -> Option<Range<usize>> {
        extract_preceding_decorator_range(unwrap_export(node)?, "decorator")
    }
}

/// Check whether a node is inside an export statement.
fn is_exported(node: TsNode<'_>) -> bool {
    node.parent()
        .is_some_and(|p| p.kind() == TypeScriptLanguage::EXPORT_STATEMENT)
}

/// If `node` is inside an `export_statement`, return the export; otherwise
/// return the node itself. Used to look for preceding doc/decorator siblings
/// on the correct outer node.
fn unwrap_export(node: TsNode<'_>) -> Option<TsNode<'_>> { Some(if is_exported(node) { node.parent()? } else { node }) }

/// Extract the variable name from a `lexical_declaration` via `variable_declarator`.
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
fn strip_jsdoc(raw: &str) -> String {
    let inner = raw
        .strip_prefix("/**")
        .and_then(|s| s.strip_suffix("*/"))
        .unwrap_or(raw);
    inner
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("* ")
                .or_else(|| trimmed.strip_prefix('*'))
                .unwrap_or(trimmed)
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

register_syntax!(TypeScriptLanguage);

/// Tests for TypeScript decomposition.
#[cfg(test)]
mod tests;
