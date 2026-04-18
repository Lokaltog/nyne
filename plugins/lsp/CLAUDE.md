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

## Indexing-progress gating

Cold-start LSP queries park on a per-`Client` `ProgressTracker`
(`session/client/progress.rs`) until the server reports initial
indexing complete via `$/progress` Begin/End notifications and the
open set has been continuously empty for a configurable debounce
window. Without this gate, queries against rust-analyzer return
empty results during the first ~10-60 s after spawn.

**State machine, not a boolean** -- four states, condvar-signaled:

- `Uninitialized { open }` -- pre-`arm`. Tokens accumulate (handles the
  race between `initialize` returning and `Client::spawn` calling
  `arm`); `wait_ready` returns immediately.
- `Indexing { open, idle_since }` -- armed. `wait_ready` blocks until
  `open` has been empty since `idle_since` for at least
  `index_debounce`, or until `index_timeout` elapses. Any new `Begin`
  arriving while `idle_since` is set clears it (debounce reset), so
  back-to-back progress cycles -- like rust-analyzer's
  `Fetching` then `Indexing` -- keep the gate parked through both.
- `Ready` -- queryable. Background re-analysis after edits stays in
  `Ready` (no regression).
- `Shutdown` -- terminal, distinct from `Ready`. `Drop_for_Client`
  triggers it before issuing the LSP shutdown request so the request
  itself does not block on the gate.

**Why a debounce, not an immediate transition:** rust-analyzer emits
`rustAnalyzer/Fetching` (cargo metadata) and `rustAnalyzer/Indexing`
(semantic analysis) as separate `$/progress` cycles separated by a
sub-second gap. An immediate `note_end`-to-empty -> `Ready`
transition releases the gate between the two, and reads return empty
because semantic indexing has not started. The debounce window
absorbs the gap.

**Inline grace timer:** if `wait_ready` times out while still in
`Indexing`, the calling thread forces the transition to `Ready` so
subsequent callers do not re-pay the cost. No dedicated timer thread.

**Capability requirement:** rust-analyzer (and other progress-emitting
servers) only sends indexing `$/progress` if the client advertised
`window.work_done_progress: true` -- set in `client_capabilities()`.

**Config:** `[lsp].index_timeout` (humantime, default 120 s) bounds
the cold-start wait. `[lsp].response_timeout` (default 10 s) bounds
individual request-response cycles after the gate releases. Per-server
`[plugin.lsp.servers.<name>].index_debounce` (humantime, default 1 s)
sets the quiescence window before `Ready`.

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
