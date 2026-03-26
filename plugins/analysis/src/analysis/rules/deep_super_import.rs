//! Analysis rule: detect deep `super::` chains in `use` declarations.
//!
//! Rust-specific. Triggers when a `use` path contains `super::super::` (2+
//! levels). The idiomatic fix is `crate::` absolute paths.

use crate::TsNode;
use crate::analysis::{AnalysisRule, Hint, Severity, register_analysis_rule};

pub const ID: &str = "deep-super-import";
/// Analysis rule that detects deep `super::` import chains.
struct DeepSuperImport;

/// Tree-sitter node kinds for `use` declarations across languages.
///
/// Currently only Rust has this pattern, but the array is extensible.
const USE_DECLARATION: &[&str] = &["use_declaration"];

/// [`AnalysisRule`] implementation for `DeepSuperImport`.
impl AnalysisRule for DeepSuperImport {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { USE_DECLARATION }

    /// Checks the given node for deep super-chain import violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        let text = node.text();

        // Count consecutive `super::` segments.
        let depth = super_depth(text);
        if depth < 2 {
            return None;
        }

        Some(Hint::from_node_line(
            self,
            node,
            Severity::Warning,
            format!("`super::` repeated {depth} times — use `crate::` absolute path instead"),
            &["Replace `super::super::...` with the equivalent `crate::` path"],
        ))
    }
}

/// Count the longest chain of consecutive `super::` segments in a use path.
///
/// Handles `use super::super::foo::Bar` and grouped `use super::super::{A, B}`.
/// The first `::` segment may be prefixed with `use` / `pub use` / etc.
fn super_depth(text: &str) -> usize {
    let mut max_depth = 0;
    let mut current_depth = 0;

    for part in text.split("::") {
        let trimmed = part.trim();
        // First segment carries the `use` keyword: "use super", "pub(crate) use super", etc.
        if trimmed == "super" || trimmed.ends_with(" super") {
            current_depth += 1;
        } else {
            max_depth = max_depth.max(current_depth);
            current_depth = 0;
        }
    }
    max_depth.max(current_depth)
}

register_analysis_rule!(DeepSuperImport);

/// Tests for deep super import detection.
#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that super-chain depth counting works correctly.
    #[test]
    fn counts_depth_correctly() {
        assert_eq!(super_depth("use super::foo::Bar;"), 1);
        assert_eq!(super_depth("use super::super::foo::Bar;"), 2);
        assert_eq!(super_depth("use super::super::super::names::FOO;"), 3);
    }

    /// Verifies detection of deep super chains in grouped imports.
    #[test]
    fn handles_grouped_imports() {
        assert_eq!(super_depth("use super::super::{Foo, Bar};"), 2);
    }

    /// Verifies no false positive on crate-rooted paths.
    #[test]
    fn no_false_positive_on_crate_path() {
        assert_eq!(super_depth("use crate::foo::Bar;"), 0);
    }

    /// Verifies no false positive on single super imports.
    #[test]
    fn no_false_positive_on_single_super() {
        assert_eq!(super_depth("use super::foo::Bar;"), 1);
    }
}
