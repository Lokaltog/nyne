# nyne-plugin-analysis

Static analysis engine — tree-sitter-based code smell detection with configurable rules.

## AnyMap

- **Inserts:** `Arc<Engine>`
- **Reads:** `Arc<SyntaxRegistry>`, `DecompositionCache`

## Rule Registration

`register_analysis_rule!` macro with `linkme::distributed_slice(ANALYSIS_RULE_FACTORIES)`. New rule: create file in `engine/rules/`, implement `Rule`, call macro. Every rule module defines `pub const ID: &str` — reference these constants (not literals) in `DEFAULT_DISABLED_RULES` and `collapse_summary` (in `provider/mod.rs`).
