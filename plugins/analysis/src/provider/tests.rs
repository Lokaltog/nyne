use rstest::*;

use super::{COLLAPSE_THRESHOLD, build_view};
use crate::engine::{Hint, Severity};

/// Builds a `Hint` fixture with the given rule ID, line, and suggestions.
fn hint(rule_id: &'static str, line: usize, suggestions: &'static [&'static str]) -> Hint {
    Hint {
        rule_id,
        severity: Severity::Info,
        line_range: line..line,
        message: format!("msg at {line}"),
        suggestions,
    }
}

/// Verifies `build_view` across the full collapse/dedup/independence matrix:
/// empty input, below-threshold emission, at-threshold collapse, per-rule
/// independence (mixed rules), and cross-hit suggestion deduplication.
///
/// Case tuple: `(input_hints, hints_len, collapsed_len, suggestions_len,
/// first_hint_rule?, first_collapsed_(rule, count)?)`.
#[rstest]
#[case::empty(Vec::new(), 0, 0, 0, None, None)]
#[case::below_threshold(
    vec![hint("magic-string", 5, &["fix it"]), hint("magic-string", 10, &["fix it"])],
    2, 0, 1, None, None,
)]
#[case::at_threshold(
    vec![
        hint("magic-string", 1, &["extract"]),
        hint("magic-string", 5, &["extract"]),
        hint("magic-string", 9, &["extract"]),
    ],
    0, 1, 1, None, Some(("magic-string", COLLAPSE_THRESHOLD)),
)]
#[case::mixed_rules_independent(
    vec![
        hint("magic-string", 1, &[]),
        hint("magic-string", 2, &[]),
        hint("magic-string", 3, &[]),
        hint("unwrap-chain", 10, &["propagate"]),
    ],
    1, 1, 1, Some("unwrap-chain"), Some(("magic-string", 3)),
)]
#[case::duplicate_suggestions(
    vec![
        hint("magic-number", 1, &["extract const", "add comment"]),
        hint("magic-number", 2, &["extract const"]),
        hint("magic-number", 3, &["extract const", "add comment"]),
    ],
    0, 1, 2, None, None,
)]
fn build_view_cases(
    #[case] hints: Vec<Hint>,
    #[case] hints_len: usize,
    #[case] collapsed_len: usize,
    #[case] suggestions_len: usize,
    #[case] first_hint_rule: Option<&str>,
    #[case] first_collapsed: Option<(&str, usize)>,
) {
    let view = build_view(&hints);
    assert_eq!(view.hints.len(), hints_len, "hints count");
    assert_eq!(view.collapsed.len(), collapsed_len, "collapsed count");
    assert_eq!(view.suggestions.len(), suggestions_len, "suggestions count");
    if let Some(rule) = first_hint_rule {
        assert_eq!(view.hints[0].rule_id, rule);
    }
    if let Some((rule, count)) = first_collapsed {
        assert_eq!(view.collapsed[0].rule_id, rule);
        assert_eq!(view.collapsed[0].count, count);
    }
}
