//! Python language decomposer.

use std::ops::Range;

use super::prelude::*;

/// File extensions handled by the Python decomposer — SSOT for both
/// syntax and LSP language registration.
pub const EXTENSIONS: &[&str] = &["py"];

/// Python language specification for tree-sitter decomposition.
struct PythonLanguage;

/// Constants for Python tree-sitter node kinds.
impl PythonLanguage {
    /// Tree-sitter node kind for comments.
    const COMMENT: &str = "comment";
    /// Tree-sitter node kind for concatenated string literals.
    const CONCATENATED_STRING: &str = "concatenated_string";
    /// Tree-sitter node kind for expression statements (wraps assignments, docstrings, etc.).
    const EXPRESSION_STATEMENT: &str = "expression_statement";
    /// Tree-sitter node kind for simple string literals.
    const STRING: &str = "string";
}

impl LanguageSpec for PythonLanguage {
    /// File extensions for Python.
    const EXTENSIONS: &'static [&'static str] = EXTENSIONS;
    /// AST node kinds that represent imports.
    const IMPORT_KINDS: &'static [&'static str] = &["import_statement", "import_from_statement"];
    /// Language name identifier.
    const NAME: &'static str = "Python";
    const RECURSABLE_KINDS: &'static [&'static str] = &["class_definition"];

    symbol_map! {
        "function_definition"  => Function,
        "class_definition"     => Class,
        "type_alias_statement" => TypeAlias,
    }

    /// Returns the tree-sitter grammar.
    fn grammar(_ext: &str) -> tree_sitter::Language { tree_sitter_python::LANGUAGE.into() }

    /// Unwraps a decorated definition to expose the inner symbol.
    fn unwrap_wrapper(node: TsNode<'_>) -> Option<(TsNode<'_>, WrapperInfo)> {
        if node.kind() != "decorated_definition" {
            return None;
        }
        let inner = node.field("definition")?;
        let decorator_range = extract_decorator_range_from_decorated(node);
        Some((inner, WrapperInfo {
            visibility: None,
            decorator_range,
        }))
    }

    /// Builds a display signature for the symbol.
    fn build_signature(node: TsNode<'_>, _kind: SymbolKind) -> String { node.first_line().to_owned() }

    /// Extracts non-standard symbols like assignments.
    fn extract_extra(node: TsNode<'_>, _remaining_depth: usize, parent_name: Option<&str>) -> Option<Fragment> {
        match node.kind() {
            Self::EXPRESSION_STATEMENT => node
                .children()
                .into_iter()
                .find(|c| c.kind() == "assignment")
                .and_then(|assignment| build_assignment_fragment(assignment, node, parent_name)),
            "assignment" => build_assignment_fragment(node, node, parent_name),
            _ => None,
        }
    }

    /// Extracts the docstring range for a symbol.
    fn extract_doc_range(node: TsNode<'_>) -> Option<Range<usize>> { extract_body_docstring_range(node) }

    /// Extracts the module-level docstring range.
    fn extract_file_doc_range(root: TsNode<'_>) -> Option<Range<usize>> {
        let first_stmt = root
            .children()
            .into_iter()
            .find(|child| child.kind() != Self::COMMENT)?;
        if first_stmt.kind() != Self::EXPRESSION_STATEMENT {
            return None;
        }
        first_stmt.children().into_iter().find(|inner| {
            (inner.kind() == Self::STRING || inner.kind() == Self::CONCATENATED_STRING)
                && is_triple_quoted(inner.text())
        })?;
        // Return the expression_statement range (matches extract_body_docstring_range).
        Some(first_stmt.byte_range())
    }

    fn strip_doc_comment(raw: &str) -> String {
        if is_triple_quoted(raw) {
            let (content, _) = strip_triple_quotes(raw);
            dedent_docstring(content)
        } else {
            raw.to_owned()
        }
    }

    /// Wraps text in doc comment syntax.
    fn wrap_doc_comment(plain: &str, indent: &str) -> String {
        let mut result = String::from("\"\"\"");
        result.push_str(plain);
        if plain.contains('\n') {
            result.push('\n');
            result.push_str(indent);
        }
        result.push_str("\"\"\"");
        result
    }
}

/// Extract the byte range of decorator nodes within a `decorated_definition`.
///
/// Unlike Rust attributes (which are preceding siblings), Python decorators
/// are *children* of the `decorated_definition` wrapper node. This function
/// scans those children to find the first and last `decorator` node and
/// returns the spanning range. The range is used to create a `Decorator`
/// child fragment in the VFS.
fn extract_decorator_range_from_decorated(decorated_node: TsNode<'_>) -> Option<Range<usize>> {
    let mut first: Option<usize> = None;
    let mut last_end: Option<usize> = None;
    for child in decorated_node.children() {
        if child.kind() == "decorator" {
            if first.is_none() {
                first = Some(child.start_byte());
            }
            last_end = Some(child.end_byte());
        }
    }
    Some(first?..last_end?)
}

/// Extract the docstring range from inside a function/class body (PEP 257).
///
/// Python docstrings are the first expression statement in a function or
/// class body, provided that expression is a triple-quoted string literal.
/// Comments before the first statement are skipped (they are not docstrings).
///
/// Returns the byte range of the enclosing `expression_statement` node
/// (not the string node itself), matching the convention used by
/// `extract_file_doc_range` for consistency in splice operations.
fn extract_body_docstring_range(node: TsNode<'_>) -> Option<Range<usize>> {
    let body = node.body()?;
    let first_stmt = body
        .children()
        .into_iter()
        .find(|child| child.kind() != PythonLanguage::COMMENT)?;
    if first_stmt.kind() != PythonLanguage::EXPRESSION_STATEMENT {
        return None;
    }
    let is_docstring = first_stmt.children().into_iter().any(|inner| {
        (inner.kind() == PythonLanguage::STRING || inner.kind() == PythonLanguage::CONCATENATED_STRING)
            && is_triple_quoted(inner.text())
    });
    is_docstring.then(|| first_stmt.byte_range())
}

/// Build a variable fragment from an assignment node.
///
/// `assignment` is the inner `assignment` node (provides `left` field for
/// the variable name). `range_node` is the outer node whose byte range
/// becomes the fragment span -- this may be the same node, or the enclosing
/// `expression_statement` when the assignment is wrapped.
fn build_assignment_fragment(
    assignment: TsNode<'_>,
    range_node: TsNode<'_>,
    parent_name: Option<&str>,
) -> Option<Fragment> {
    let name = assignment.field("left").map(|n| n.text().to_owned())?;
    let signature = range_node.first_line().to_owned();
    let name_offset = assignment
        .field("left")
        .map_or_else(|| range_node.start_byte(), |n| n.start_byte());

    Some(build_code_fragment(
        range_node,
        CodeFragmentSpec {
            name,
            kind: SymbolKind::Variable,
            signature,
            name_byte_offset: name_offset,
            visibility: None,
            children: Vec::new(),
        },
        parent_name,
    ))
}

/// Skip valid Python string prefix characters (`r`, `b`, `u`, and combinations).
///
/// Python allows 1- or 2-character prefixes before the quote delimiter
/// (e.g. `r"..."`, `rb"..."`, `Rb"..."`). This strips those so downstream
/// functions can check for triple-quote delimiters directly.
fn skip_string_prefix(s: &str) -> &str {
    let bytes = s.as_bytes();
    let prefix_len = match bytes {
        [b'r' | b'R' | b'b' | b'B' | b'u' | b'U', b'r' | b'R' | b'b' | b'B', ..] => 2,
        [b'r' | b'R' | b'b' | b'B' | b'u' | b'U', ..] => 1,
        _ => 0,
    };
    &s[prefix_len..]
}

/// Check whether a string starts with triple quotes.
fn is_triple_quoted(text: &str) -> bool {
    let after = skip_string_prefix(text);
    after.starts_with("\"\"\"") || after.starts_with("'''")
}

/// Strip triple-quote delimiters from a docstring.
fn strip_triple_quotes(s: &str) -> (&str, &str) {
    let after_prefix = skip_string_prefix(s);
    for quote in &["\"\"\"", "'''"] {
        if let Some(rest) = after_prefix.strip_prefix(quote) {
            let inner = rest.strip_suffix(quote).unwrap_or(rest);
            return (inner, quote);
        }
    }
    (s, "\"\"\"")
}

/// Dedent a Python docstring following PEP 257 conventions.
///
/// The first line is always stripped of leading whitespace independently.
/// For subsequent lines, the minimum indentation across all non-empty lines
/// is removed uniformly. Trailing blank lines are stripped.
///
/// This matches the algorithm described in PEP 257's "Handling Docstring
/// Indentation" section and produces clean output for VFS `docstring.txt`.
fn dedent_docstring(content: &str) -> String {
    if content.is_empty() {
        return String::new();
    }

    let lines: Vec<&str> = content.lines().collect();
    #[allow(clippy::indexing_slicing)] // lines is non-empty (content is non-empty)
    let first = lines[0].trim_start();
    let rest = lines.get(1..).unwrap_or(&[]);
    if rest.is_empty() {
        return first.to_owned();
    }

    let min_indent = rest
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);

    let mut result = String::new();
    if !first.is_empty() {
        result.push_str(first);
    }

    for line in rest {
        result.push('\n');
        if !line.trim().is_empty() {
            result.push_str(line.get(min_indent..).unwrap_or_else(|| line.trim_start()));
        }
    }

    result.trim_end_matches('\n').to_owned()
}

register_syntax!(PythonLanguage);

#[cfg(test)]
mod tests;
