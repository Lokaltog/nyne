# nyne-plugin-coding

Coding plugin — syntax decomposition, LSP integration, batch editing, developer-experience features. Module structure discoverable via VFS.

## Dependencies

- **nyne** (core): Provider trait, dispatch types, templates, node abstractions
- **nyne-plugin-git** (non-optional): `GitRepo` accessed via TypeMap for symbol-scoped git features

## Cross-Crate Consumers

`CodingServices` and selected types are `pub` for downstream plugin crates (`nyne-plugin-claude`). Public surface:

- `services::CodingServices` — bundle of syntax, LSP, decomposition, analysis services
- `syntax::` — `SyntaxRegistry`, `DecomposedSource`, `Fragment`, `find_fragment_at_line`, `AnalysisEngine`, `AnalysisContext`, `HintView`, `fragment_list`, template partials
- `lsp::` — `LspManager`, `FileQuery`, `DiagnosticRow`, `diagnostics_to_rows`
- `providers::names` — VFS name constants, `symbol_from_vfs_path`, `is_symbols_overview`

## CodingServices

`services.rs` — consolidated bundle of all plugin services. During `activate()`, a single `CodingServices` struct is inserted into the TypeMap containing: `Arc<SyntaxRegistry>`, `Arc<LspManager>`, `DecompositionCache`, `Arc<AnalysisEngine>`, `CodingConfig`. Internal provider code retrieves services via `CodingServices::get(ctx)`. External plugins use `CodingServices::try_get(ctx)` for optional access.

`PassthroughProcesses` is still inserted separately (consumed by core, not by plugin providers).

## Config

Plugin config: `[plugin.coding]` in `~/.config/nyne/config.toml` or project-level `.nyne/config.toml` / `.nyne.toml` / `nyne.toml`.

Config is multi-tier: plugin defaults → user config → project config. Merged via `deep_merge` (arrays concatenated, objects recursive-merged). Plugin defaults are provided by `CodingPlugin::default_config()`.

- `config/mod.rs`: `CodingConfig` — deserialized via `CodingConfig::from_plugin_config()`
- `config/lsp.rs`: `LspConfig`, `ServerEntry`, `LanguageIdMapping` — LSP server definitions
- `[plugin.coding.lsp.servers]`: array of server entries (name, command, args, extensions, language_ids, root_markers, enabled). Built-in defaults in `default_servers()`. See `lsp/CLAUDE.md` for config examples.
- `[plugin.coding.analysis]`: `enabled` (bool), `rules` (optional list — `None` = all except `DEFAULT_DISABLED_RULES`)
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
