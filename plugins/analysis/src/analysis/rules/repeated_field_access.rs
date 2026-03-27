//! Analysis rule: detect repeated field access chains that should use a local binding.
//!
//! Triggers when `MIN_REPETITIONS` (3) or more consecutive statements share
//! the same field-access prefix (e.g. `self.config.timeout`), suggesting the
//! prefix should be bound to a local variable.
//!
//! **Why it matters:** Repeated long access chains add visual noise and make
//! refactoring harder. A local binding like `let cfg = &self.config;` reduces
//! repetition and clarifies intent.
//!
//! **Example trigger:**
//! ```rust
//! self.config.timeout = Duration::from_secs(30);
//! self.config.retries = 3;
//! self.config.verbose = true;
//! // Prefer: let cfg = &mut self.config;
//! ```

use super::kinds;
use crate::TsNode;
use crate::analysis::{Hint, Rule, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "repeated-field-access";
/// Minimum consecutive sibling statements sharing the same field-access prefix.
const MIN_REPETITIONS: usize = 3;

/// Analysis rule that detects repeated field access chains.
struct RepeatedFieldAccess;

/// [`Rule`] implementation for `RepeatedFieldAccess`.
impl Rule for RepeatedFieldAccess {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::FUNCTION }

    /// Checks the given node for repeated field access violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        let body = node.raw().child_by_field_name("body")?;
        let source = node.source();
        let mut cursor = body.walk();
        let children: Vec<_> = body.named_children(&mut cursor).collect();

        let (start_line, end_line, prefix) = find_longest_run(&children, source)?;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Info,
            line_range: start_line..end_line,
            message: format!("Repeated `{prefix}` access across consecutive statements"),
            suggestions: &["Bind the repeated prefix to a local variable"],
        })
    }
}

/// Find the longest consecutive run of siblings sharing a field-access prefix.
/// Returns `(start_line, end_line, prefix)` if a run of ≥ `MIN_REPETITIONS` exists.
fn find_longest_run<'a>(children: &[tree_sitter::Node<'_>], source: &'a [u8]) -> Option<(usize, usize, &'a str)> {
    let mut best: Option<(usize, usize, &'a str)> = None;
    let mut i = 0;

    while let Some(child) = children.get(i) {
        let Some(prefix) = receiver_prefix(child, source) else {
            i += 1;
            continue;
        };

        let start = i;
        while children
            .get(i)
            .and_then(|c| receiver_prefix(c, source))
            .is_some_and(|p| p == prefix)
        {
            i += 1;
        }

        let run_len = i - start;
        if run_len >= MIN_REPETITIONS && best.is_none_or(|(_, _, prev)| run_len > prev.len()) {
            let start_line = children.get(start).map_or(0, |c| c.start_position().row);
            let end_line = children.get(i.wrapping_sub(1)).map_or(0, |c| c.end_position().row);
            best = Some((start_line, end_line, prefix));
        }
    }

    best
}

/// Extract a two-segment receiver prefix like `self.foo.bar` from a statement's
/// leading expression. Returns `None` if the statement doesn't start with a
/// field access chain of depth ≥ 2.
fn receiver_prefix<'a>(node: &tree_sitter::Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let expr = if node.kind() == kinds::EXPRESSION_STATEMENT {
        node.named_child(0)?
    } else {
        *node
    };

    let mut current = expr;
    loop {
        if kinds::FIELD_ACCESS.contains(&current.kind()) {
            // Rust: `value:`, JS/TS: `object:`
            let object = current
                .child_by_field_name("value")
                .or_else(|| current.child_by_field_name("object"))?;
            if kinds::FIELD_ACCESS.contains(&object.kind()) {
                return kinds::node_str(&object, source);
            }
            return None;
        }
        if let Some(child) = current
            .child_by_field_name("function")
            .or_else(|| current.child_by_field_name("left"))
            .or_else(|| current.child_by_field_name("value"))
            .or_else(|| current.named_child(0))
        {
            current = child;
        } else {
            return None;
        }
    }
}

register_analysis_rule!(RepeatedFieldAccess);
