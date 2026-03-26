//! Analysis rule: detect ASCII art separators.

use super::kinds::{self, strip_comment_prefix};
use crate::TsNode;
use crate::analysis::{AnalysisContext, AnalysisRule, Hint, Severity, register_analysis_rule};

/// Minimum consecutive separator characters for a pure separator line.
const MIN_PURE_RUN: usize = 3;

/// Minimum consecutive separator characters in a header separator (text flanked by runs).
/// Lower than pure because `── Title ──` is unambiguous even with a 2-char run.
const MIN_HEADER_RUN: usize = 2;

/// Characters that form ASCII art separator lines.
const SEPARATOR_CHARS: &[char] = &['-', '=', '─', '═', '━', '~', '*'];

/// Analysis rule that detects ASCII art separator lines in comments.
struct AsciiSeparator;

/// [`AnalysisRule`] implementation for `AsciiSeparator`.
impl AnalysisRule for AsciiSeparator {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { "ascii-separator" }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::COMMENT }

    /// Checks the given node for ASCII art separator violations.
    fn check(&self, node: TsNode<'_>, _context: &AnalysisContext<'_>) -> Option<Hint> {
        let text = node.text();

        // Check each line of the comment (block comments can span multiple lines).
        let is_separator = text.lines().any(|line| {
            let stripped = strip_comment_prefix(line);
            is_pure_separator(stripped) || is_header_separator(stripped)
        });

        if !is_separator {
            return None;
        }

        let start_line = node.raw().start_position().row;
        let end_line = node.raw().end_position().row;

        Some(Hint {
            rule_id: self.id(),
            severity: Severity::Warning,
            line_range: start_line..end_line,
            message: "ASCII separator line detected — remove it and use doc comments or blank lines for structure"
                .into(),
            suggestions: vec![
                "Delete the separator line".into(),
                "Replace with a doc comment describing the section".into(),
            ],
        })
    }
}

/// Pure separator: the entire content is separator characters, at least `MIN_PURE_RUN`.
/// e.g. `--------`, `═══════════`, `~~~`
fn is_pure_separator(content: &str) -> bool {
    let (chars, bytes) = separator_run(content);
    chars >= MIN_PURE_RUN && bytes == content.len()
}

/// Header separator: separator run, text, separator run.
/// e.g. `--- Section Name ---`, `=== Module ===`
fn is_header_separator(content: &str) -> bool {
    let (leading_chars, leading_bytes) = separator_run(content);
    if leading_chars < MIN_HEADER_RUN {
        return false;
    }
    let rest = content[leading_bytes..].trim();
    if rest.is_empty() {
        return false;
    }
    // Check for trailing separator run (only need char count for comparison).
    let trailing = rest.chars().rev().take_while(|c| SEPARATOR_CHARS.contains(c)).count();
    trailing >= MIN_HEADER_RUN
}

/// Count consecutive separator characters from the start.
/// Returns (`char_count`, `byte_length`) to handle multi-byte chars safely.
fn separator_run(s: &str) -> (usize, usize) {
    let mut chars = 0;
    let mut bytes = 0;
    for c in s.chars() {
        if !SEPARATOR_CHARS.contains(&c) {
            break;
        }
        chars += 1;
        bytes += c.len_utf8();
    }
    (chars, bytes)
}

register_analysis_rule!(AsciiSeparator);
