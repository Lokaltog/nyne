# nyne-plugin-source

Source plugin — syntax decomposition, batch editing, developer-experience features. Module structure discoverable via VFS.

## Dependencies

- **nyne** (core): Provider trait, dispatch types, templates, node abstractions
- **nyne-plugin-git** (non-optional): `GitRepo` accessed via TypeMap for symbol-scoped git features

## Cross-Crate Consumers

`SourceServices` and selected types are `pub` for downstream plugin crates (`nyne-plugin-claude`). Public surface:

- `services::SourceServices` — bundle of syntax and decomposition services
- `syntax::` — `SyntaxRegistry`, `TsNode`, `DecomposedSource`, `Fragment`, `find_fragment_at_line`, `fragment_list`, template partials
- `providers::well_known` — VFS name constants, `symbol_from_vfs_path`, `is_symbols_overview`
- `providers::syntax::FileRenameHook` — trait for external file-rename coordination (implemented by LSP plugin)

## SourceServices

`services.rs` — consolidated bundle of all plugin services. During `activate()`, a single `SourceServices` struct is inserted into the TypeMap containing: `Arc<SyntaxRegistry>`, `DecompositionCache`, `SourceConfig`. Internal provider code retrieves services via `SourceServices::get(ctx)`.

## Config

Plugin config: `[plugin.source]` in `~/.config/nyne/config.toml` or project-level `.nyne/config.toml` / `.nyne.toml` / `nyne.toml`.

Config is multi-tier: plugin defaults → user config → project config. Merged via `deep_merge` (arrays concatenated, objects recursive-merged). Plugin defaults are provided by `SourcePlugin::default_config()`.

## routes! Macro

Full reference in `nyne/src/providers/CLAUDE.md`. Source-specific notes:

- `BatchEditProvider` keeps two separate route trees (`at_routes` for `@/edit/`, `companion_routes` for per-symbol `edit/`)

## FragmentResolver

`providers/fragment_resolver.rs` — lazy handle for accessing decomposed source files. All content readers and splice writers hold a clone. **Never capture `SymbolLineRange` or `Arc<DecomposedSource>` on a `Readable`/`TemplateView` type** — use `FragmentResolver` instead, which resolves at call time.
