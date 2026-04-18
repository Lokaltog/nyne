# nyne-plugin-lsp

## AnyMap

- **Inserts:** `Arc<Manager>`, `PassthroughProcesses`, `Arc<LspState>`
- **Reads:** `Arc<SyntaxRegistry>`, `DecompositionCache`, `Arc<SourcePaths>`

## LspQuery scope

`LspQuery` (in `session/handle.rs`) pairs an `Arc<Handle>` with a stored LSP `Range` and a `Position` anchor. Three scopes, constructed via `Handle`:

- **Point** — `Handle::at(source, byte_offset)`. Zero-width range. Used for hover / references / rename.
- **Range** — `Handle::over(source, byte_range)` / `Handle::over_lines(line_range)`. Used for code actions / inlay hints. Defaults `position = range.start`; override with `LspQuery::with_position(source, name_offset)` to anchor positional queries at a symbol's name token.
- **File** — `Handle::whole_file()`. Maximal `0:0..MAX:MAX` range, no source text required — works for any LSP-backed file, decomposed or not.

All scopes route through the same `Feature::query(&LspQuery)`, `actions::resolve_code_actions(&LspQuery)`, and `find_action_diff` — no scope-specific fork.

## Code actions

Exposed at two scopes, reusing the same `CodeActionDiff` `DiffSource`:

- **Symbol-level** — `file.rs@/symbols/Foo@/actions/NN-*.diff` (wired in `register_source_extensions`, built via `LspState::resolve_actions_dir`).
- **File-level** — `file.rs@/actions/NN-*.diff` (wired in `register_companion_extensions`, built via `LspState::resolve_file_actions_dir`).

Both paths gate on the server's `code_action_provider` capability.

## LspState operations

All stateful LSP orchestrations live as methods on `LspState` (not free functions). Shared helpers:

- `fragment_query` — build an `LspQuery` anchored at a symbol's name token, scoped to its line extent.
- `resolve_fragment_context` (inlined into `fragment_query`)
- `resolve_symlink_dir` — feature-dir contents (callers/, refs/, ...).
- `resolve_actions_dir` / `resolve_file_actions_dir` — symbol-scoped / file-scoped code actions.
- `query_targets` — `Feature::from_dir_name` + `Feature::query` dispatch.
- `search_nodes` — workspace symbol search results.
- `diagnostics_node` — `DIAGNOSTICS.md` template node.

Pure compute helpers stay in `lsp_links/` as module-private free functions (no `SourceCtx` indirection — take explicit `&SyntaxRegistry` / `&DecompositionCache` / `symbols_dir` params).
