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

/// Fixture with hint count below the collapse threshold.
#[fixture]
fn below_threshold() -> Vec<Hint> {
    vec![
        hint("magic-string", 5, &["fix it"]),
        hint("magic-string", 10, &["fix it"]),
    ]
}

/// Fixture with hint count at the collapse threshold.
#[fixture]
fn at_threshold() -> Vec<Hint> {
    vec![
        hint("magic-string", 1, &["extract"]),
        hint("magic-string", 5, &["extract"]),
        hint("magic-string", 9, &["extract"]),
    ]
}

/// Fixture with hints from multiple rules at varying counts.
#[fixture]
fn mixed_rules() -> Vec<Hint> {
    vec![
        hint("magic-string", 1, &[]),
        hint("magic-string", 2, &[]),
        hint("magic-string", 3, &[]),
        hint("unwrap-chain", 10, &["propagate"]),
    ]
}

/// Fixture with duplicate suggestion text across multiple hints.
#[fixture]
fn duplicate_suggestions() -> Vec<Hint> {
    vec![
        hint("magic-number", 1, &["extract const", "add comment"]),
        hint("magic-number", 2, &["extract const"]),
        hint("magic-number", 3, &["extract const", "add comment"]),
    ]
}

/// Verifies that below-threshold hints emit individual rows.
#[rstest]
fn below_threshold_emits_individual_rows(below_threshold: Vec<Hint>) {
    let view = build_view(&below_threshold);
    assert_eq!(view.hints.len(), 2);
    assert!(view.collapsed.is_empty());
    // Same suggestion text deduped to one entry.
    assert_eq!(view.suggestions.len(), 1);
}

/// Verifies that at-threshold hints collapse into a summary row.
#[rstest]
fn at_threshold_collapses_into_summary(at_threshold: Vec<Hint>) {
    let view = build_view(&at_threshold);
    assert!(view.hints.is_empty());
    assert_eq!(view.collapsed.len(), 1);
    assert_eq!(view.collapsed[0].count, COLLAPSE_THRESHOLD);
    assert_eq!(view.collapsed[0].rule_id, "magic-string");
    // Suggestions still emitted (deduplicated).
    assert_eq!(view.suggestions.len(), 1);
}

/// Verifies that each rule collapses independently of others.
#[rstest]
fn mixed_rules_collapse_independently(mixed_rules: Vec<Hint>) {
    let view = build_view(&mixed_rules);
    // magic-string collapsed, unwrap-chain stays individual.
    assert_eq!(view.hints.len(), 1);
    assert_eq!(view.hints[0].rule_id, "unwrap-chain");
    assert_eq!(view.collapsed.len(), 1);
    assert_eq!(view.collapsed[0].rule_id, "magic-string");
}

/// Verifies that duplicate suggestions are deduplicated across hits.
#[rstest]
fn suggestions_deduplicated_across_hits(duplicate_suggestions: Vec<Hint>) {
    let view = build_view(&duplicate_suggestions);
    // Two unique texts for magic-number, regardless of how many hits.
    assert_eq!(view.suggestions.len(), 2);
}

/// Verifies that an empty hint list produces empty output.
#[test]
fn empty_hints_returns_empty() {
    let view = build_view(&[]);
    assert!(view.hints.is_empty());
    assert!(view.collapsed.is_empty());
    assert!(view.suggestions.is_empty());
}
