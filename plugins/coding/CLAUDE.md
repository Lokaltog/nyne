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

## routes! Macro

Full reference in `nyne/src/providers/CLAUDE.md`. Coding-specific notes:

- `WorkspaceSearchProvider` uses `"{query}"` capture for `@/search/symbols/{query}` — workspace symbol search via LSP
- `BatchEditProvider` keeps two separate route trees (`at_routes` for `@/edit/`, `companion_routes` for per-symbol `edit/`)

## LSP URI ↔ Path

- **SSOT:** `lsp::uri::uri_to_file_path` — converts `lsp_types::Uri` → `PathBuf` (strips `file://` prefix)
- **SSOT:** `lsp::uri::file_path_to_uri` — converts `Path` → `lsp_types::Uri` (inverse)
- `lsp_types::Uri` has no `to_file_path()` method — always use the SSOT functions above
- In tests: construct via `"file:///path".parse::<lsp_types::Uri>()`

## LspFeature — Slug-Derived Names

`LspFeature` in `providers/syntax/content/lsp/feature.rs` is the SSOT for all per-symbol LSP features. Each variant declares only a **slug** (e.g. `"type_definition"`). All names are derived via `convert_case`:

- `file_name`: `slug.to_case(UpperKebab) + ".md"` → `TYPE-DEFINITION.md`
- `dir_name`: `slug.to_case(Kebab)` → `type-definition`
- template key/source: `concat!` from slug (compile-time)
- template global: `FILE_{slug.to_case(UpperSnake)}` → `file_name` (auto-registered)

Adding a new LSP feature: one `meta!("slug")` arm + `is_supported` arm + `query` arm + `.j2` template file. No constants to declare, no template globals to register manually.

## FragmentResolver

`providers/fragment_resolver.rs` — lazy handle for accessing decomposed source files. All content readers and splice writers hold a clone. **Never capture `SymbolLineRange` or `Arc<DecomposedSource>` on a `Readable`/`TemplateView` type** — use `FragmentResolver` instead, which resolves at call time.
