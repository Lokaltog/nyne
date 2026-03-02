//! Analysis rule: detect deep `super::` chains in `use` declarations.
//!
//! Rust-specific. Triggers when a `use` path contains `super::super::` (2+
//! levels). The idiomatic fix is `crate::` absolute paths.

use crate::syntax::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};
use crate::syntax::parser::TsNode;

struct DeepSuperImport;

/// Tree-sitter node kinds for `use` declarations across languages.
///
/// Currently only Rust has this pattern, but the array is extensible.
const USE_DECLARATION: &[&str] = &["use_declaration"];

impl AnalysisRule for DeepSuperImport {
    fn id(&self) -> &'static str { "deep-super-import" }

    fn node_kinds(&self) -> &'static [&'static str] { USE_DECLARATION }

    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let text = node.text();

        // Count consecutive `super::` segments.
        let depth = super_depth(text);
        if depth < 2 {
            return None;
        }

        let line = node.raw().start_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Warning,
            line_range: line..line,
            message: format!("`super::` repeated {depth} times — use `crate::` absolute path instead"),
            suggestions: vec!["Replace `super::super::...` with the equivalent `crate::` path".into()],
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_depth_correctly() {
        assert_eq!(super_depth("use super::foo::Bar;"), 1);
        assert_eq!(super_depth("use super::super::foo::Bar;"), 2);
        assert_eq!(super_depth("use super::super::super::names::FOO;"), 3);
    }

    #[test]
    fn handles_grouped_imports() {
        assert_eq!(super_depth("use super::super::{Foo, Bar};"), 2);
    }

    #[test]
    fn no_false_positive_on_crate_path() {
        assert_eq!(super_depth("use crate::foo::Bar;"), 0);
    }

    #[test]
    fn no_false_positive_on_single_super() {
        assert_eq!(super_depth("use super::foo::Bar;"), 1);
    }
}
