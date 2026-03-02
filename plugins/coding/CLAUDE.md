# nyne-plugin-coding

Coding plugin — syntax decomposition, LSP integration, AI-assisted editing, developer-experience features. Module structure discoverable via VFS.

## Dependencies

- **nyne** (core): Provider trait, dispatch types, templates, node abstractions
- **nyne-plugin-git** (non-optional): `GitRepo` accessed via TypeMap for symbol-scoped git features

## TypeMap Contributions

During `activate()`, inserts: `Arc<SyntaxRegistry>`, `Arc<LspManager>`, `DecompositionCache`, `Arc<AnalysisEngine>`, `PassthroughProcesses`.

## Config

Plugin config: `[plugin.coding]` in `~/.config/nyne/config.toml`. LSP config: `[lsp]` (top-level, `ctx.config().lsp`).

- `config.rs`: `CodingConfig` + `AnalysisConfig` + `ClaudeConfig` — deserialized via `CodingConfig::from_plugin_table()`
- `[plugin.coding.analysis]`: `enabled` (bool), `rules` (optional list — `None` = all except `DEFAULT_DISABLED_RULES`)
- `[plugin.coding.claude]`: `enabled` (bool, default true) — master toggle for `.claude/` directory and all hooks
- `[plugin.coding.claude.hooks]`: per-hook toggles — `session_start`, `pre_tool_use`, `post_tool_use`, `stop`, `statusline` (all default true)
- `DEFAULT_DISABLED_RULES` in `syntax/analysis/mod.rs` is the SSOT for default-excluded rules

## FragmentResolver

`providers/fragment_resolver.rs` — lazy handle for accessing decomposed source files. All content readers and splice writers hold a clone. **Never capture `SymbolLineRange` or `Arc<DecomposedSource>` on a `Readable`/`TemplateView` type** — use `FragmentResolver` instead, which resolves at call time.
