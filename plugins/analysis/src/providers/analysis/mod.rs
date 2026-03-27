//! Static code analysis rendering for `ANALYSIS.md`.
//!
//! Runs nyne's built-in analysis engine at read time and collapses
//! high-frequency rules into summary groups to keep output readable.

use std::collections::{HashMap, HashSet};

use nyne::prelude::*;
use nyne::templates::TemplateEngine;
use nyne_source::providers::fragment_resolver::FragmentResolver;
use serde::Serialize;

use crate::{Engine, Hint, HintView};

/// View for `ANALYSIS.md` — runs static analysis at read time.
///
/// Surfaces code-quality suggestions (magic numbers, single-use variables, etc.)
/// from nyne's built-in analysis engine. Analysis is run lazily on each read so
/// results always reflect current source.
pub struct Content {
    pub resolver: FragmentResolver,
    pub activation: Arc<ActivationContext>,
}

/// [`TemplateView`] implementation for [`Content`].
impl TemplateView for Content {
    /// Run analysis and render hints via template.
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let shared = self.resolver.decompose()?;

        let (hint_rows, collapsed, suggestions) = {
            let hints: Vec<Hint> = shared
                .tree
                .as_ref()
                .and_then(|tree| Some(self.activation.get::<Arc<Engine>>()?.analyze(tree, &shared.source)))
                .unwrap_or_default();
            build_view(&hints)
        };

        let view = minijinja::context! {
            hints => hint_rows,
            collapsed => collapsed,
            suggestions => suggestions,
        };
        Ok(engine.render_bytes(template, &view))
    }
}

/// A group of hints collapsed into a single summary when count exceeds a threshold.
#[derive(Serialize)]
struct CollapsedGroup {
    rule_id: &'static str,
    severity: &'static str,
    count: usize,
    /// Representative message (first occurrence, without the specific value).
    summary: &'static str,
}

/// Threshold: rules with this many or more hits get collapsed into a summary row.
const COLLAPSE_THRESHOLD: usize = 3;

/// A suggestion row for analysis hints.
#[derive(Serialize)]
struct SuggestionRow {
    rule_id: &'static str,
    text: &'static str,
}

/// Rule-level summary messages used when collapsing repeated hits.
fn collapse_summary(rule_id: &str) -> &'static str {
    use crate::engine::rules::{magic_number, magic_string, redundant_clone, single_use_variable, unwrap_chain};

    match rule_id {
        magic_string::ID => "multiple magic strings — extract to named constants for clarity",
        magic_number::ID => "multiple magic numbers — extract to named constants for clarity",
        single_use_variable::ID => "multiple single-use bindings — consider inlining",
        unwrap_chain::ID => "multiple `.unwrap()` chains — consider propagating errors",
        redundant_clone::ID => "multiple redundant `.clone()` calls",
        _ => "multiple occurrences",
    }
}

/// Build the hints view, collapsing repeated rules above [`COLLAPSE_THRESHOLD`].
///
/// Returns three collections for the template: individual hint rows (for
/// low-frequency rules), collapsed summary groups (for noisy rules), and
/// deduplicated suggestion rows. This prevents a single rule like
/// `single-use-variable` from flooding the output with repetitive entries.
fn build_view(hints: &[Hint]) -> (Vec<HintView>, Vec<CollapsedGroup>, Vec<SuggestionRow>) {
    // Count occurrences per rule to decide what gets collapsed.
    let counts: HashMap<&'static str, usize> = hints.iter().fold(HashMap::new(), |mut acc, h| {
        *acc.entry(h.rule_id).or_default() += 1;
        acc
    });

    let is_collapsed = |id: &str| counts.get(id).copied().unwrap_or(0) >= COLLAPSE_THRESHOLD;

    // Individual rows for low-frequency rules only.
    let rows: Vec<HintView> = hints
        .iter()
        .filter(|h| !is_collapsed(h.rule_id))
        .map(HintView::from)
        .collect();

    // One summary row per high-frequency rule (first occurrence wins for severity).
    let mut seen = HashSet::new();
    let collapsed: Vec<CollapsedGroup> = hints
        .iter()
        .filter(|h| is_collapsed(h.rule_id) && seen.insert(h.rule_id))
        .map(|h| CollapsedGroup {
            rule_id: h.rule_id,
            severity: h.severity.into(),
            count: counts.get(h.rule_id).copied().unwrap_or(0),
            summary: collapse_summary(h.rule_id),
        })
        .collect();

    // Deduplicated suggestions: one entry per unique (rule_id, text) pair.
    let mut seen_suggestions: HashSet<(&str, &str)> = HashSet::new();
    let suggestions: Vec<SuggestionRow> = hints
        .iter()
        .flat_map(|h| h.suggestions.iter().map(move |s| (h.rule_id, *s)))
        .filter(|(rule_id, text)| seen_suggestions.insert((rule_id, text)))
        .map(|(rule_id, text)| SuggestionRow { rule_id, text })
        .collect();

    (rows, collapsed, suggestions)
}

/// Unit tests.
#[cfg(test)]
mod tests;
