//! Analysis rule: detect deeply nested generic types.
//!
//! Triggers when a type annotation nests generic parameters deeper than
//! `MAX_TYPE_DEPTH` (3), e.g. `HashMap<String, Vec<Option<Arc<Mutex<T>>>>>`.
//!
//! **Why it matters:** Deeply nested generics are hard to read and often signal
//! that a type alias or newtype wrapper would improve clarity.
//!
//! **Example trigger:**
//! ```rust
//! fn process(data: HashMap<String, Vec<Option<Result<T, E>>>>) { .. }
//! // Prefer: type DataMap = HashMap<String, Vec<Option<Result<T, E>>>>;
//! ```
//!
//! **Caveat:** Skips type alias declarations (`type Foo = ...`) since those
//! are themselves the fix for this smell.

use super::kinds;
use crate::TsNode;
use crate::engine::{Hint, Rule, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "deeply-nested-type";
/// Maximum nesting depth for generic type parameters.
const MAX_TYPE_DEPTH: usize = 3;

/// Analysis rule that detects deeply nested generic types.
struct DeeplyNestedType;

/// [`Rule`] implementation for `DeeplyNestedType`.
impl Rule for DeeplyNestedType {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::TYPE_ANNOTATION }

    /// Checks the given node for deeply nested generic type violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        let raw = node.raw();

        // Only trigger on the outermost type — skip if parent is also a type.
        // Also skip if already inside a type alias — suggesting "extract a type
        // alias" for the RHS of an existing alias is nonsensical.
        if let Some(parent) = raw.parent()
            && (kinds::TYPE_ANNOTATION.contains(&parent.kind())
                || kinds::GENERIC_TYPE.contains(&parent.kind())
                || kinds::GENERIC_TYPE_ARGS.contains(&parent.kind())
                || kinds::TYPE_ALIAS.contains(&parent.kind()))
        {
            return None;
        }

        let depth = max_generic_depth(raw);
        if depth < MAX_TYPE_DEPTH {
            return None;
        }

        Some(Hint::from_node_line(
            self,
            node,
            Severity::Info,
            format!(
                "Type `{}` has {depth} levels of nesting",
                kinds::node_str(&raw, node.source()).unwrap_or("(complex type)")
            ),
            &["Extract a type alias"],
        ))
    }
}

/// Compute the maximum nesting depth of generic type parameters.
///
/// Only `GENERIC_TYPE` nodes (the `<...>` levels) count as nesting.
/// `TYPE_ANNOTATION` nodes (plain type names) are traversed but don't
/// add depth — `Vec<String>` is depth 1, not 3.
fn max_generic_depth(node: tree_sitter::Node<'_>) -> usize {
    let mut max_child_depth = 0;
    let mut cursor = node.walk();

    for child in node.named_children(&mut cursor) {
        let kind = child.kind();
        if kinds::GENERIC_TYPE.contains(&kind)
            || kinds::GENERIC_TYPE_ARGS.contains(&kind)
            || kinds::TYPE_ANNOTATION.contains(&kind)
        {
            max_child_depth = max_child_depth.max(max_generic_depth(child));
        }
    }

    if kinds::GENERIC_TYPE.contains(&node.kind()) {
        1 + max_child_depth
    } else {
        max_child_depth
    }
}

register_analysis_rule!(DeeplyNestedType);
