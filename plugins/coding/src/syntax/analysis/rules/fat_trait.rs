//! Analysis rule: detect traits with too many required methods.
//!
//! Fat traits are hard to implement, hard to mock, and usually conflate
//! multiple responsibilities. Suggests splitting into focused sub-traits.

use super::kinds;
use crate::syntax::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};
use crate::syntax::parser::TsNode;

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

struct FatTrait;

impl AnalysisRule for FatTrait {
    fn id(&self) -> &'static str { "fat-trait" }

    fn node_kinds(&self) -> &'static [&'static str] { TRAIT_DEF }

    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let raw = node.raw();

        let body = raw.child_by_field_name("body").unwrap_or(raw);
        let mut cursor = body.walk();

        let required_count = body
            .named_children(&mut cursor)
            .filter(|c| REQUIRED_METHOD.contains(&c.kind()))
            .count();

        if required_count <= MAX_REQUIRED_METHODS {
            return None;
        }

        let name = trait_name(raw, node.source()).unwrap_or("(anonymous)");
        let start_line = raw.start_position().row;
        let end_line = raw.end_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Warning,
            line_range: start_line..end_line,
            message: format!(
                "Trait `{name}` has {required_count} required methods (threshold: {MAX_REQUIRED_METHODS})"
            ),
            suggestions: vec![
                "Split into smaller, focused traits".into(),
                "Provide default implementations where possible".into(),
            ],
        })
    }
}

fn trait_name<'a>(node: tree_sitter::Node<'_>, source: &'a [u8]) -> Option<&'a str> {
    let name_node = node.child_by_field_name("name")?;
    kinds::node_str(&name_node, source)
}

register_analysis_rule!(FatTrait);
