//! Analysis rule: detect magic strings.

use super::kinds;
use crate::syntax::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};
use crate::syntax::parser::TsNode;

/// Additional safe parents specific to string literals.
const STRING_SAFE_PARENTS: &[&str] = &[
    "macro_invocation",
    "token_tree",
    "attribute_item",
    "attribute",
    "decorator",
    "use_declaration",
    "import_statement",
    "call_expression",
];

/// Minimum string length to flag. Short strings ("", "x", ",") are rarely magic.
const MIN_LENGTH: usize = 4;

/// Analysis rule that detects magic strings.
struct MagicString;

/// [`AnalysisRule`] implementation for `MagicString`.
impl AnalysisRule for MagicString {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { "magic-string" }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::STRING }

    /// Checks the given node for magic string violations.
    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let text = node.text();

        // Strip quotes to get the content.
        let content = strip_string_quotes(text);
        if content.len() < MIN_LENGTH {
            return None;
        }

        // Skip strings that look like format patterns, paths, or URLs.
        if looks_like_format_or_path(content) {
            return None;
        }

        let ancestor = node.raw().parent()?;
        if kinds::is_safe_literal_context(ancestor, STRING_SAFE_PARENTS) {
            return None;
        }

        let start_line = node.raw().start_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Info,
            line_range: start_line..start_line,
            message: format!("Magic string `{text}` — extract to a named constant for clarity"),
            suggestions: vec!["Extract to a `const` or `static` with a descriptive name".into()],
        })
    }
}

/// Strip outer quotes from a string literal.
fn strip_string_quotes(s: &str) -> &str {
    // Handle raw strings (r"..." / r#"..."#).
    let s = s.strip_prefix('r').unwrap_or(s);
    let s = s.trim_start_matches('#');
    let s = s.strip_prefix('"').unwrap_or(s);
    let s = s.strip_prefix('\'').unwrap_or(s);
    let s = s.strip_suffix('"').unwrap_or(s);
    let s = s.strip_suffix('\'').unwrap_or(s);
    s.trim_end_matches('#')
}

/// Strings with format placeholders, paths, or URLs are typically intentional.
fn looks_like_format_or_path(content: &str) -> bool {
    content.contains("{}") || content.contains("{0") || content.starts_with('/') || content.contains("://")
}

register_analysis_rule!(MagicString);
