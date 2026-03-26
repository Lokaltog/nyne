# nyne-plugin-lsp

LSP integration — language server lifecycle, diagnostics, workspace search, and LSP-powered VFS nodes. Module structure discoverable via VFS.

## Dependencies

- **nyne** (core): Provider trait, dispatch types, templates
- **nyne-plugin-source**: `SyntaxRegistry`, `DecompositionCache`, `FragmentResolver`, `DiffActionNode`, `FileEditResult`

## Architecture

The LSP plugin contributes additional nodes to symbol directories owned by the source plugin's `SyntaxProvider` via multi-provider composition. The dispatch layer auto-merges directory children from multiple providers.

Progressive disclosure: when the LSP plugin is loaded, symbol directories gain LSP-powered nodes (CALLERS.md, DEPS.md, REFERENCES.md, rename/, actions/, DIAGNOSTICS.md). Without it, only base syntax nodes appear.

## TypeMap

- **Inserts:** `Arc<LspManager>`, `PassthroughProcesses`, `Arc<dyn FileRenameHook>`
- **Reads:** `Arc<SyntaxRegistry>` (from source plugin)

## Config

Plugin config: `[plugin.lsp]` in `~/.config/nyne/config.toml` or project-level config.

- `config.rs`: `LspConfig`, `ServerEntry`, `LanguageIdMapping`
- `[plugin.lsp.servers]`: array of server entries (name, command, args, extensions, language_ids, root_markers, enabled)
- Built-in defaults in `default_servers()`

## LSP URI ↔ Path

- **SSOT:** `lsp::uri::uri_to_file_path` — converts `lsp_types::Uri` → `PathBuf`
- **SSOT:** `lsp::uri::file_path_to_uri` — converts `Path` → `lsp_types::Uri`
- In tests: construct via `"file:///path".parse::<lsp_types::Uri>()`
