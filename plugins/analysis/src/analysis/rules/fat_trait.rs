//! Analysis rule: detect traits with too many required methods.
//!
//! Fat traits are hard to implement, hard to mock, and usually conflate
//! multiple responsibilities. Suggests splitting into focused sub-traits.

use super::kinds;
use crate::TsNode;
use crate::analysis::{Hint, Rule, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "fat-trait";
/// Maximum required methods before triggering.
const MAX_REQUIRED_METHODS: usize = 8;

/// Tree-sitter node kinds for trait/interface declarations.
const TRAIT_DEF: &[&str] = &[
    "trait_item",                 // Rust
    "interface_declaration",      // TypeScript
    "abstract_class_declaration", // TypeScript
];

/// Node kinds that represent required (bodyless) method signatures.
const REQUIRED_METHOD: &[&str] = &[
    "function_signature_item", // Rust: `fn foo(&self);` in a trait
    "method_signature",        // TypeScript interface methods
];

/// Analysis rule that detects traits with too many required methods.
struct FatTrait;

/// [`Rule`] implementation for `FatTrait`.
impl Rule for FatTrait {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { TRAIT_DEF }

    /// Checks the given node for fat trait violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        let raw = node.raw();
        let required_count = kinds::count_children_of_kind(&raw, "body", REQUIRED_METHOD);

        if required_count <= MAX_REQUIRED_METHODS {
            return None;
        }

        let name = trait_name(raw, node.source()).unwrap_or("(anonymous)");

        Some(Hint::from_node(
            self,
            node,
            Severity::Warning,
            format!("Trait `{name}` has {required_count} required methods (threshold: {MAX_REQUIRED_METHODS})"),
            &[
                "Split into smaller, focused traits",
                "Provide default implementations where possible",
            ],
        ))
    }
}

/// Extract the name of a trait from its tree-sitter node.
fn trait_name<'a>(node: tree_sitter::Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let name_node = node.child_by_field_name("name")?;
    kinds::node_str(&name_node, source)
}

register_analysis_rule!(FatTrait);
