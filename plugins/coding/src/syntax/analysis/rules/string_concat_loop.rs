//! Analysis rule: detect string concatenation in loops.

use super::kinds;
use crate::syntax::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};
use crate::syntax::parser::TsNode;

/// Analysis rule that detects string concatenation in loops.
struct StringConcatLoop;

/// [`AnalysisRule`] implementation for `StringConcatLoop`.
impl AnalysisRule for StringConcatLoop {
    fn id(&self) -> &'static str { "string-concat-loop" }

    fn node_kinds(&self) -> &'static [&'static str] { kinds::LOOP }

    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let body = node.raw().child_by_field_name("body")?;

        if !has_string_concat(body, node.source()) {
            return None;
        }

        let start_line = node.raw().start_position().row;
        let end_line = node.raw().end_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Warning,
            line_range: start_line..end_line,
            message: "String concatenation inside loop — use a buffer or collect/join instead".into(),
            suggestions: vec![
                "Use `String::with_capacity()` + `push_str()` or `Vec::join()`".into(),
                "Collect into a `Vec<&str>` and `.join()` after the loop".into(),
            ],
        })
    }
}

/// Recursively check if a block contains string concatenation via `+=`.
fn has_string_concat(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let mut cursor = node.walk();

    for child in node.named_children(&mut cursor) {
        let kind = child.kind();

        // Check for compound assignment: `s += "..."`
        if (kind == "compound_assignment_expr" || kind == "augmented_assignment") && is_concat_assignment(child, source)
        {
            return true;
        }

        // Check for `x = x + "..."` pattern.
        if (kind == "assignment_expression" || kind == "assignment") && is_reassign_concat(child, source) {
            return true;
        }

        // Don't recurse into nested functions.
        if !kinds::FUNCTION.contains(&kind) && has_string_concat(child, source) {
            return true;
        }
    }
    false
}

/// Check if a compound assignment is `+= <string-ish>`.
fn is_concat_assignment(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let text = node_text(&node, source);
    if !text.contains("+=") {
        return false;
    }
    rhs_has_string(node, source)
}

/// Check if `x = x + "..."` pattern.
fn is_reassign_concat(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let Some(right) = node
        .child_by_field_name("right")
        .or_else(|| node.child_by_field_name("value"))
    else {
        return false;
    };

    if right.kind() != "binary_expression" && right.kind() != "binary_operator" {
        return false;
    }

    let text = node_text(&right, source);
    if !text.contains('+') {
        return false;
    }

    subtree_has_string(right, source)
}

/// Check if RHS of a compound assignment involves a string.
fn rhs_has_string(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    let right = node
        .child_by_field_name("right")
        .or_else(|| node.child_by_field_name("value"));
    match right {
        Some(r) => subtree_has_string(r, source),
        None => false,
    }
}

/// Check if any node in the subtree is a string literal.
fn subtree_has_string(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    if kinds::STRING.contains(&node.kind()) {
        return true;
    }

    // Also match method calls like `.to_string()`, `str()`.
    if node.kind() == "call_expression" {
        let text = node_text(&node, source);
        if text.contains("to_string") || text.contains("to_owned") {
            return true;
        }
    }

    let mut cursor = node.walk();
    node.named_children(&mut cursor).any(|c| subtree_has_string(c, source))
}

/// Extract the UTF-8 text of a tree-sitter node from source bytes.
fn node_text<'a>(node: &tree_sitter::Node<'_>, source: &'a [u8]) -> &'a str {
    use std::str::from_utf8;
    source
        .get(node.byte_range())
        .and_then(|b| from_utf8(b).ok())
        .unwrap_or("")
}

register_analysis_rule!(StringConcatLoop);
