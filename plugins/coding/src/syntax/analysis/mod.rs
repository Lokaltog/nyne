//! Code analysis engine for inspecting parse trees and generating hints.
//!
//! Analysis rules are registered at compile time and dispatched by node kind
//! for O(1) lookup. Rules produce [`Hint`]s that suggest code improvements.

mod rules;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use nyne::dispatch::activation::ActivationContext;

use super::parser::TsNode;
use crate::config::AnalysisConfig;

/// Rules disabled by default because they tend to be noisy on most codebases.
///
/// Users can override this by setting `rules = []` (all rules) or listing
/// specific rules in `[plugin.coding.analysis]`.
pub(crate) const DEFAULT_DISABLED_RULES: &[&str] = &[
    "magic-string",
    "magic-number",
    "single-use-variable",
    // TODO:
    // a. type-in-variable-name — Fix the matching logic
    // Bug: name.contains(frag) does substring matching, so storage_strategy matches _str (a substring of _strategy). Fix: split the variable name by _
    // into segments and check if any segment exactly matches a type name (str, string, vec, map, etc.) instead of substring searching. This eliminates
    // false matches on compound words.
    // b. string-format-push — Disable by default
    // Problem: flags format!("...: {}", foo.display()) as "use push_str" — but you can't use push_str with impl Display types. Tree-sitter can't do type
    // analysis, so this will always false-flag on non-&str format args. Add "string-format-push" to DEFAULT_DISABLED_RULES.
    // c. redundant-clone — Disable by default
    // Problem: flags .clone() on references (e.g., |g| g.0.clone() where g is &T from TypeMap::get()). Without type information, the rule can't
    // distinguish "clone needed to convert &T → T" from "unnecessary clone of owned value." Add "redundant-clone" to DEFAULT_DISABLED_RULES.
    "type-in-variable-name",
    "string-format-push",
    "redundant-clone",
];

/// Factory function that creates analysis rule instances at startup.
pub type AnalysisRuleFactory = fn() -> Vec<Box<dyn AnalysisRule>>;

#[linkme::distributed_slice]
pub(crate) static ANALYSIS_RULE_FACTORIES: [AnalysisRuleFactory];

/// Register one or more analysis rules for link-time auto-discovery.
///
/// # Examples
///
/// ```ignore
/// register_analysis_rule!(DeepNesting);
/// register_analysis_rule!(DeepNesting, LongParameterList);
/// ```
macro_rules! register_analysis_rule {
    ($($rule:expr),+ $(,)?) => {
        #[allow(unsafe_code)]
        #[linkme::distributed_slice($crate::syntax::analysis::ANALYSIS_RULE_FACTORIES)]
        static _ANALYSIS_RULE: $crate::syntax::analysis::AnalysisRuleFactory = || {
            vec![$(Box::new($rule)),+]
        };
    };
}
pub(crate) use register_analysis_rule;

/// Severity level for analysis hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::AsRefStr)]
#[strum(serialize_all = "lowercase")]
pub enum Severity {
    /// Informational hint — useful context, not a problem.
    Info,
    /// Warning — likely code smell worth addressing.
    Warning,
}

/// A hint produced by an analysis rule.
#[derive(Debug, Clone)]
pub struct Hint {
    pub rule_id: &'static str,
    pub severity: Severity,
    pub line_range: Range<usize>,
    pub message: String,
    pub suggestions: Vec<String>,
}

/// Serializable view of a [`Hint`] for template rendering.
///
/// Converts 0-based `line_range` to 1-based `line_start`/`line_end` and
/// pre-formats `severity` as a string. This is the single source of truth
/// for the `Hint → template context` conversion — used by both the
/// `HINTS.md` provider and the `PostToolUse` hook.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HintView {
    pub rule_id: &'static str,
    pub severity: &'static str,
    pub message: String,
    pub line_start: usize,
    pub line_end: usize,
    pub suggestions: Vec<String>,
}

impl From<&Hint> for HintView {
    fn from(hint: &Hint) -> Self {
        Self {
            rule_id: hint.rule_id,
            severity: hint.severity.as_ref(),
            message: hint.message.clone(),
            line_start: hint.line_range.start + 1,
            line_end: hint.line_range.end + 1,
            suggestions: hint.suggestions.clone(),
        }
    }
}

/// Context provided to analysis rules during inspection.
pub struct AnalysisContext<'a> {
    pub source: &'a str,
    pub activation: &'a ActivationContext,
}

/// A rule that inspects tree-sitter nodes and produces hints.
///
/// Rules declare which node kinds they're interested in via [`Self::node_kinds`].
/// The analysis engine only calls [`Self::check`] for matching nodes — no
/// per-rule full-tree walks.
pub trait AnalysisRule: Send + Sync {
    /// Unique identifier for this rule (e.g. `"deep-nesting"`).
    fn id(&self) -> &'static str;

    /// Tree-sitter node kinds this rule wants to inspect.
    ///
    /// The engine dispatches nodes to rules based on this list.
    /// Return `&[]` to receive *all* nodes (expensive — avoid if possible).
    fn node_kinds(&self) -> &'static [&'static str];

    /// Inspect a node and optionally produce a hint.
    fn check(&self, node: TsNode<'_>, context: &AnalysisContext<'_>) -> Option<Hint>;
}

type RuleVec = Vec<Arc<dyn AnalysisRule>>;
type DispatchMap = HashMap<&'static str, RuleVec>;

/// Collected analysis rules, indexed by node kind for O(1) dispatch.
pub struct AnalysisEngine {
    /// Rules keyed by the node kinds they handle.
    dispatch: DispatchMap,
    /// Rules that want all nodes (empty `node_kinds`).
    catch_all: RuleVec,
}

impl AnalysisEngine {
    /// Build the engine from all registered rule factories.
    ///
    /// Cheap to construct — just indexes rule factories into a dispatch map.
    /// Callers own the instance; no global state, no locking.
    pub fn build() -> Self {
        let rules: Vec<Arc<dyn AnalysisRule>> = ANALYSIS_RULE_FACTORIES
            .iter()
            .flat_map(|factory| factory())
            .map(Arc::from)
            .collect();

        Self::from_rules(rules)
    }

    /// Build the engine, activating only the rules permitted by `config`.
    ///
    /// - `enabled = false` → empty engine (no rules run).
    /// - `rules = None` (absent from config) → all rules except [`DEFAULT_DISABLED_RULES`].
    /// - `rules = Some([])` (explicit empty) → all registered rules.
    /// - `rules = Some(set)` → only matching rule IDs.
    ///
    /// Unknown rule names in `config.rules` produce a warning at startup.
    pub(crate) fn build_filtered(config: &AnalysisConfig) -> Self {
        if !config.enabled {
            return Self {
                dispatch: DispatchMap::new(),
                catch_all: RuleVec::new(),
            };
        }

        let all_rules: RuleVec = ANALYSIS_RULE_FACTORIES
            .iter()
            .flat_map(|factory| factory())
            .map(Arc::from)
            .collect();

        let Some(rules) = &config.rules else {
            // No `rules` key in config → apply default exclusions.
            let filtered = all_rules
                .into_iter()
                .filter(|r| !DEFAULT_DISABLED_RULES.contains(&r.id()))
                .collect();
            return Self::from_rules(filtered);
        };

        if rules.is_empty() {
            // Explicit `rules = []` → all rules, no exclusions.
            return Self::from_rules(all_rules);
        }

        // Warn about unknown rule names so typos are caught early.
        let known: HashSet<&str> = all_rules.iter().map(|r| r.id()).collect();
        for name in rules {
            if !known.contains(name.as_str()) {
                tracing::warn!(rule = %name, "unknown analysis rule in config — ignored");
            }
        }

        let filtered = all_rules.into_iter().filter(|r| rules.contains(r.id())).collect();

        Self::from_rules(filtered)
    }

    /// Index a set of rules into the dispatch map.
    fn from_rules(rules: RuleVec) -> Self {
        let mut dispatch = DispatchMap::new();
        let mut catch_all = RuleVec::new();

        for rule in rules {
            let kinds = rule.node_kinds();
            if kinds.is_empty() {
                catch_all.push(rule);
                continue;
            }
            let unique_kinds: HashSet<&str> = kinds.iter().copied().collect();
            for kind in unique_kinds {
                dispatch.entry(kind).or_default().push(Arc::clone(&rule));
            }
        }

        Self { dispatch, catch_all }
    }

    /// Analyze a parsed tree, returning all hints.
    ///
    /// Performs a single depth-first walk of the tree, dispatching each node
    /// to interested rules by kind. O(nodes), not O(nodes × rules).
    pub fn analyze(&self, tree: &tree_sitter::Tree, context: &AnalysisContext<'_>) -> Vec<Hint> {
        let mut hints = Vec::new();
        let mut cursor = tree.walk();
        let source = context.source.as_bytes();

        self.walk_recursive(&mut cursor, source, context, &mut hints);

        hints
    }

    fn walk_recursive(
        &self,
        cursor: &mut tree_sitter::TreeCursor<'_>,
        source: &[u8],
        context: &AnalysisContext<'_>,
        hints: &mut Vec<Hint>,
    ) {
        let node = TsNode::new(cursor.node(), source);

        self.check_node(node, context, hints);

        // Recurse into children.
        if !cursor.goto_first_child() {
            return;
        }
        loop {
            self.walk_recursive(cursor, source, context, hints);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }

    /// Dispatch a single node to all interested rules.
    fn check_node(&self, node: TsNode<'_>, context: &AnalysisContext<'_>, hints: &mut Vec<Hint>) {
        let kind = node.kind();

        // Kind-specific rules.
        if let Some(rules) = self.dispatch.get(kind) {
            hints.extend(rules.iter().filter_map(|r| r.check(node, context)));
        }

        // Catch-all rules.
        hints.extend(self.catch_all.iter().filter_map(|r| r.check(node, context)));
    }
}
