# nyne-plugin-analysis

Static analysis engine — tree-sitter-based code smell detection with configurable rules.

## Dependencies

- **nyne** (core): `ActivationContext`, `TypeMap`, plugin trait
- **nyne-plugin-source**: `TsNode`, `SyntaxRegistry`, `DecompositionCache`, `FragmentResolver`

## Architecture

The analysis plugin is a **leaf plugin** — it depends on source but nothing depends on it (claude uses it as an optional feature dependency). It inserts `Arc<Engine>` into the TypeMap during `activate()`. Consumers (claude hooks, future analysis provider) read it from TypeMap.

## TypeMap

- **Inserts:** `Arc<Engine>`
- **Reads:** nothing (self-contained after activation)

## Config

Plugin config: `[plugin.analysis]` in `~/.config/nyne/config.toml` or project-level config.

- `config.rs`: `Config` — `enabled` (bool), `rules` (optional list)
- `DEFAULT_DISABLED_RULES` in `engine/mod.rs` is the SSOT for default-excluded rules

## Rule Registration

Rules use `register_analysis_rule!` macro with `linkme::distributed_slice(ANALYSIS_RULE_FACTORIES)`. Adding a new rule: create a file in `engine/rules/`, implement `Rule`, call `register_analysis_rule!(MyRule)`.

Every rule module must define `pub const ID: &str = "rule-name"` and return it from `id()`. Reference these constants (not string literals) in `DEFAULT_DISABLED_RULES` and `collapse_summary`.
