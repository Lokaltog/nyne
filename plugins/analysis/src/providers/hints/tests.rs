use nyne_analysis::analysis::{Hint, Severity};
use rstest::*;

use super::{COLLAPSE_THRESHOLD, build_view};

/// Builds a `Hint` fixture with the given rule ID, line, and suggestions.
fn hint(rule_id: &'static str, line: usize, suggestions: Vec<String>) -> Hint {
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
        hint("magic-string", 5, vec!["fix it".into()]),
        hint("magic-string", 10, vec!["fix it".into()]),
    ]
}

/// Fixture with hint count at the collapse threshold.
#[fixture]
fn at_threshold() -> Vec<Hint> {
    vec![
        hint("magic-string", 1, vec!["extract".into()]),
        hint("magic-string", 5, vec!["extract".into()]),
        hint("magic-string", 9, vec!["extract".into()]),
    ]
}

/// Fixture with hints from multiple rules at varying counts.
#[fixture]
fn mixed_rules() -> Vec<Hint> {
    vec![
        hint("magic-string", 1, vec![]),
        hint("magic-string", 2, vec![]),
        hint("magic-string", 3, vec![]),
        hint("unwrap-chain", 10, vec!["propagate".into()]),
    ]
}

/// Fixture with duplicate suggestion text across multiple hints.
#[fixture]
fn duplicate_suggestions() -> Vec<Hint> {
    vec![
        hint("magic-number", 1, vec!["extract const".into(), "add comment".into()]),
        hint("magic-number", 2, vec!["extract const".into()]),
        hint("magic-number", 3, vec!["extract const".into(), "add comment".into()]),
    ]
}

/// Verifies that below-threshold hints emit individual rows.
#[rstest]
fn below_threshold_emits_individual_rows(below_threshold: Vec<Hint>) {
    let (rows, collapsed, suggestions) = build_view(&below_threshold);
    assert_eq!(rows.len(), 2);
    assert!(collapsed.is_empty());
    // Same suggestion text deduped to one entry.
    assert_eq!(suggestions.len(), 1);
}

/// Verifies that at-threshold hints collapse into a summary row.
#[rstest]
fn at_threshold_collapses_into_summary(at_threshold: Vec<Hint>) {
    let (rows, collapsed, suggestions) = build_view(&at_threshold);
    assert!(rows.is_empty());
    assert_eq!(collapsed.len(), 1);
    assert_eq!(collapsed[0].count, COLLAPSE_THRESHOLD);
    assert_eq!(collapsed[0].rule_id, "magic-string");
    // Suggestions still emitted (deduplicated).
    assert_eq!(suggestions.len(), 1);
}

/// Verifies that each rule collapses independently of others.
#[rstest]
fn mixed_rules_collapse_independently(mixed_rules: Vec<Hint>) {
    let (rows, collapsed, _) = build_view(&mixed_rules);
    // magic-string collapsed, unwrap-chain stays individual.
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].rule_id, "unwrap-chain");
    assert_eq!(collapsed.len(), 1);
    assert_eq!(collapsed[0].rule_id, "magic-string");
}

/// Verifies that duplicate suggestions are deduplicated across hits.
#[rstest]
fn suggestions_deduplicated_across_hits(duplicate_suggestions: Vec<Hint>) {
    let (_, _, suggestions) = build_view(&duplicate_suggestions);
    // Two unique texts for magic-number, regardless of how many hits.
    assert_eq!(suggestions.len(), 2);
}

/// Verifies that an empty hint list produces empty output.
#[test]
fn empty_hints_returns_empty() {
    let (rows, collapsed, suggestions) = build_view(&[]);
    assert!(rows.is_empty());
    assert!(collapsed.is_empty());
    assert!(suggestions.is_empty());
}
