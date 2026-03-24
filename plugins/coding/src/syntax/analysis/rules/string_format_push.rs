//! Analysis rule: detect string formatting before push.

use super::kinds;
use crate::syntax::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};
use crate::syntax::parser::TsNode;

/// Analysis rule that detects format! used as `push_str` argument.
struct StringFormatPush;

/// [`AnalysisRule`] implementation for `StringFormatPush`.
impl AnalysisRule for StringFormatPush {
    fn id(&self) -> &'static str { "string-format-push" }

    fn node_kinds(&self) -> &'static [&'static str] { kinds::MACRO_INVOCATION }

    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
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

        let line = raw.start_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Info,
            line_range: line..line,
            message: "`format!()` used for simple concatenation without formatting".into(),
            suggestions: vec!["Use `push_str()` or string concatenation".into()],
        })
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
