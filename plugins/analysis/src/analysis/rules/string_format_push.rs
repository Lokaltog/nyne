//! Analysis rule: detect `format!()` used only as a `push_str` argument.
//!
//! Triggers on macro invocations like `s.push_str(&format!("x={x}"))` where
//! `write!(s, "x={x}")` would avoid an intermediate allocation.
//!
//! **Why it matters:** `format!()` allocates a temporary `String` that is
//! immediately consumed by `push_str` and dropped. `write!` or `write_fmt`
//! writes directly into the target buffer, saving one allocation per call.
//!
//! **Example trigger:**
//! ```rust
//! output.push_str(&format!("count: {}", n));
//! // Prefer: write!(output, "count: {}", n).unwrap();
//! ```
//!
//! **Caveat:** Only detects the `format!(...)` inside `push_str()` pattern.
//! Does not flag standalone `format!` or other allocation-then-consume patterns.

use super::kinds;
use crate::TsNode;
use crate::analysis::{Hint, Rule, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "string-format-push";
/// Analysis rule that detects format! used as `push_str` argument.
struct StringFormatPush;

/// [`Rule`] implementation for `StringFormatPush`.
impl Rule for StringFormatPush {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::MACRO_INVOCATION }

    /// Checks the given node for string formatting before push violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        let raw = node.raw();
        let source = node.source();

        // Must be `format!` macro.
        let macro_name = raw.child_by_field_name("macro")?;
        if kinds::node_bytes(&macro_name, source) != b"format" {
            return None;
        }

        // Get the token tree (arguments).
        let args = raw.named_children(&mut raw.walk()).nth(1)?;
        let args_text = kinds::node_str(&args, source)?;

        // Check if the format string only uses `{}` placeholders (pure concatenation).
        // Extract the format string — first string literal in the arguments.
        if !is_pure_concatenation(args_text) {
            return None;
        }

        Some(Hint::from_node_line(
            self,
            node,
            Severity::Info,
            "`format!()` used for simple concatenation without formatting".into(),
            &["Use `push_str()` or string concatenation"],
        ))
    }
}

/// Check if a format! invocation is pure concatenation —
/// only `{}` placeholders, no format specifiers like `{:?}`, `{:.2}`, `{name}`.
fn is_pure_concatenation(args_text: &str) -> bool {
    // Strip surrounding parens/brackets.
    let inner = args_text
        .trim()
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(args_text);

    // Must have at least one `{}` and at least one comma (i.e., interpolated values).
    if !inner.contains("{}") || !inner.contains(',') {
        return false;
    }

    // No format specifiers: reject `{:`, `{name`, `{0`, etc.
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            match chars.peek() {
                Some('}') => {
                    chars.next();
                }
                Some('{') => {
                    chars.next(); // escaped `{{`
                }
                _ => return false, // format specifier
            }
        }
    }
    true
}

register_analysis_rule!(StringFormatPush);
