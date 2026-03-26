//! Analysis rule: detect magic strings outside constant contexts.
//!
//! Triggers on string literals (4+ characters) outside constant declarations,
//! attribute annotations, and other safe contexts. Short strings, format
//! templates, file paths, and URLs are excluded as they are rarely "magic."
//!
//! **Why it matters:** Repeated string literals across a codebase lead to
//! typo-driven bugs and make refactoring fragile. Named constants centralize
//! the definition and enable IDE-assisted renaming.
//!
//! **Example trigger:**
//! ```rust
//! if role == "admin" {
//!     ..
//! } // Prefer: const ADMIN_ROLE: &str = "admin";
//! ```
//!
//! **Caveat:** Disabled by default (`DEFAULT_DISABLED_RULES`) because string
//! literals in logging, error messages, and test assertions are common and
//! produce high false-positive rates.

use super::kinds;
use crate::TsNode;
use crate::analysis::{AnalysisRule, Hint, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "magic-string";
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
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::STRING }

    /// Checks the given node for magic string violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
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

        Some(Hint::from_node_line(
            self,
            node,
            Severity::Info,
            format!("Magic string `{text}` — extract to a named constant for clarity"),
            &["Extract to a `const` or `static` with a descriptive name"],
        ))
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
