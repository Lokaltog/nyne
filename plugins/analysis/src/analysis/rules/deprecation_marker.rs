//! Analysis rule: detect deprecation markers in comments.
//!
//! Triggers on comments containing patterns like `DEPRECATED`, `LEGACY`,
//! `@deprecated`, `HACK:`, `TECH DEBT`, and similar markers that indicate
//! code slated for removal or rewrite.
//!
//! **Why it matters:** Deprecation markers often linger indefinitely. Surfacing
//! them as analysis hints keeps them visible for cleanup or migration planning.
//!
//! **Example triggers:**
//! ```text
//! // DEPRECATED: use new_api() instead
//! // HACK: workaround for upstream bug #123
//! // TODO(tech-debt): remove after v2 migration
//! ```

use super::kinds;
use crate::TsNode;
use crate::analysis::{Hint, Rule, Severity, register_analysis_rule};

/// Unique identifier for this rule, used in configuration and hint output.
pub const ID: &str = "deprecation-marker";
/// Patterns that indicate deprecated, legacy, or deferred code.
///
/// Grouped by category for maintainability. Each entry is a case-insensitive
/// substring to search for in comments and string literals.
const DEPRECATION_PATTERNS: &[(&str, &str)] = &[
    // Backwards compatibility language
    ("backwards compatibility", "backwards-compatibility language"),
    ("backward compatible", "backwards-compatibility language"),
    ("for compatibility", "backwards-compatibility language"),
    ("back-compat", "backwards-compatibility language"),
    ("compat layer", "compatibility layer"),
    ("compatibility layer", "compatibility layer"),
    // Deprecation markers
    ("@deprecated", "deprecation marker"),
    ("deprecated", "deprecation marker"),
    ("will be removed", "deprecation marker"),
    ("slated for removal", "deprecation marker"),
    ("to be removed", "deprecation marker"),
    // Partial/deferred implementation
    ("deferred to", "deferred implementation"),
    ("future implementation", "deferred implementation"),
    ("implement later", "deferred implementation"),
    ("placeholder for", "deferred implementation"),
    ("stub for", "deferred implementation"),
    ("not yet implemented", "deferred implementation"),
    ("will be implemented", "deferred implementation"),
    // Legacy/migration shim language
    ("legacy", "legacy code"),
    ("shim", "migration shim"),
    ("wrapper for old", "legacy wrapper"),
    ("migrate later", "deferred migration"),
    ("old api", "legacy API"),
    // Kept-for-reference patterns
    ("old implementation", "kept-for-reference code"),
    ("previous version", "kept-for-reference code"),
    ("kept for reference", "kept-for-reference code"),
    ("original version", "kept-for-reference code"),
    // Future work deferral
    ("phase 2", "future work deferral"),
    ("different phase", "future work deferral"),
    ("later milestone", "future work deferral"),
    ("future sprint", "future work deferral"),
    ("next version", "future work deferral"),
];

/// Analysis rule that detects deprecation markers in comments.
struct DeprecationMarker;

/// [`Rule`] implementation for `DeprecationMarker`.
impl Rule for DeprecationMarker {
    /// Returns the rule identifier.
    fn id(&self) -> &'static str { ID }

    /// Returns the tree-sitter node kinds this rule applies to.
    fn node_kinds(&self) -> &'static [&'static str] { kinds::COMMENT }

    /// Checks the given node for deprecation marker violations.
    fn check(&self, node: TsNode<'_>) -> Option<Hint> {
        let text = node.text().to_ascii_lowercase();

        let (pattern, category) = DEPRECATION_PATTERNS.iter().find(|(pat, _)| text.contains(pat))?;

        Some(Hint::from_node_line(
            self,
            node,
            Severity::Warning,
            format!("Detected {category}: `{pattern}` — remove or address this code"),
            &[
                "Remove deprecated/legacy code instead of keeping it around",
                "If still needed, create a tracking issue and remove the comment",
            ],
        ))
    }
}

register_analysis_rule!(DeprecationMarker);
