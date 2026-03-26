//! Analysis rule: detect redundant clones.

use super::kinds;
use crate::TsNode;
use crate::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};

pub const ID: &str = "redundant-clone";
/// Analysis rule that detects redundant clone calls.
struct RedundantClone;

/// [`AnalysisRule`] implementation for `RedundantClone`.
impl AnalysisRule for RedundantClone {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::CALL }

    /// Checks the given node for redundant clone violations.
    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let raw = node.raw();
        let source = node.source();

        // Must be a `.clone()` method call.
        if !is_clone_call(raw, source) {
            return None;
        }

        // The receiver — what's being cloned.
        let receiver_name = kinds::node_str(&clone_receiver(raw)?, source)?;

        // Check if receiver is used again after this call expression.
        // Walk up to the containing statement, then check later siblings.
        let stmt = containing_statement(raw);

        // If this is the last statement in a block, "not used after" is
        // trivially true — there IS no "after." The clone is almost always
        // borrowing-to-owned (MutexGuard, &self field, etc.) rather than a
        // redundant copy, and we can't distinguish without type information.
        stmt.next_named_sibling()?;

        let mut sibling = stmt.next_named_sibling();
        while let Some(s) = sibling {
            if kinds::count_identifier_uses(&s, receiver_name.as_bytes(), source) > 0 {
                return None; // Used later — clone is needed.
            }
            sibling = s.next_named_sibling();
        }

        Some(Hint::from_node_line(
            self,
            node,
            Severity::Warning,
            format!("`.clone()` on `{receiver_name}` which is not used after this point"),
            &["Remove `.clone()` — value is not used after this"],
        ))
    }
}

/// Check whether a call node is a .`clone()` or .`to_string()` invocation.
fn is_clone_call(node: tree_sitter::Node<'_>, source: &[u8]) -> bool {
    if let Some(f) = node
        .child_by_field_name("function")
        .or_else(|| node.child_by_field_name("method"))
    {
        // Rust: field_expression with field "clone"
        if let Some(field) = f.child_by_field_name("field") {
            return kinds::node_bytes(&field, source) == b"clone";
        }
        // JS/TS/Python: property access `.clone()`
        if let Some(prop) = f.child_by_field_name("property") {
            return kinds::node_bytes(&prop, source) == b"clone";
        }
    }

    // Rust: call_expression where function text ends with `.clone`
    if let Some(f) = node.child_by_field_name("function") {
        return kinds::node_bytes(&f, source).ends_with(b".clone");
    }

    false
}

/// Extract the receiver expression of a clone method call.
fn clone_receiver(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    let func = node.child_by_field_name("function")?;
    // The receiver is the `object`/`value` of the field expression.
    func.child_by_field_name("object")
        .or_else(|| func.child_by_field_name("value"))
}

/// Walk up the tree to find the enclosing statement or declaration.
fn containing_statement(mut node: tree_sitter::Node<'_>) -> tree_sitter::Node<'_> {
    while let Some(parent) = node.parent() {
        if parent.kind().ends_with("_statement")
            || parent.kind().ends_with("_declaration")
            || kinds::BINDING.contains(&parent.kind())
        {
            return parent;
        }
        node = parent;
    }
    node
}

register_analysis_rule!(RedundantClone);
