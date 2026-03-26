# nyne-plugin-analysis

Static analysis engine — tree-sitter-based code smell detection with configurable rules.

## Dependencies

- **nyne** (core): `ActivationContext`, `TypeMap`, plugin trait
- **nyne-plugin-source**: `TsNode`, `SyntaxRegistry`, `DecompositionCache`, `FragmentResolver`

## Architecture

The analysis plugin is a **leaf plugin** — it depends on source but nothing depends on it (claude uses it as an optional feature dependency). It inserts `Arc<AnalysisEngine>` into the TypeMap during `activate()`. Consumers (claude hooks, future analysis provider) read it from TypeMap.

## TypeMap

- **Inserts:** `Arc<AnalysisEngine>`
- **Reads:** nothing (self-contained after activation)

## Config

Plugin config: `[plugin.analysis]` in `~/.config/nyne/config.toml` or project-level config.

- `config.rs`: `AnalysisConfig` — `enabled` (bool), `rules` (optional list)
- `DEFAULT_DISABLED_RULES` in `analysis/mod.rs` is the SSOT for default-excluded rules

## Rule Registration

Rules use `register_analysis_rule!` macro with `linkme::distributed_slice(ANALYSIS_RULE_FACTORIES)`. Adding a new rule: create a file in `analysis/rules/`, implement `AnalysisRule`, call `register_analysis_rule!(MyRule)`.
