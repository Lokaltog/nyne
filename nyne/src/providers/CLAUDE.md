# Providers

Providers implement the `Provider` trait and produce VFS content. Core providers (this directory) are registered by dispatch; plugin providers live in `plugins/` and are discovered at link time.

For template rendering patterns and `HandleBuilder`/`TemplateHandle` workflow, see `doc/codebase.md`.

## Return Type Aliases

- **`Nodes`** = `Result<Option<Vec<VirtualNode>>>` — used by `children()`
- **`Node`** = `Result<Option<VirtualNode>>` — used by `lookup()`, `create()`, `mkdir()`

Route handler methods can return simpler types (`Vec<VirtualNode>`, `Option<VirtualNode>`) — `IntoNodes`/`IntoNode` traits wrap them automatically.

## routes! Macro Reference

### Grammar

A route tree block contains entries, each one of:

| Form | Meaning |
|-|-|
| `"segment" { ... }` | Exact segment, no handler, sub-routes in block |
| `"segment" => handler { ... }` | Exact segment with children handler + sub-routes |
| `"segment" => handler,` | Exact segment with children handler, no sub-routes |
| `no_emit "segment" ...` | Same as above, but suppresses auto-emit in parent readdir |
| `"{name}" => handler,` | Single-segment capture with children handler |
| `"{name}@" => handler,` | Capture with suffix strip (`@` removed from captured value) |
| `"PREFIX:{name}" => handler,` | Capture with prefix strip |
| `"{..name}" => handler,` | Rest capture (1+ segments) with children handler |
| `"{..name}@" => handler,` | Rest capture with suffix strip |
| `"**" => lookup(handler),` | Glob — lookup handler fires at any depth |
| `lookup(handler)` | Catch-all lookup handler on current node |
| `lookup "pattern" => handler` | Pattern-based lookup (e.g., `lookup "{ref}.diff" => handler`) |
| `children(handler)` | Children handler on root node only (error inside segment blocks — use `=>` instead) |
| `file("name", readable_expr)` | Static file with optional `.no_cache()`, `.hidden()`, `.sliceable()` |

### Dispatch: lookup vs children

**Critical distinction** — the two FUSE operations that hit a provider:

- **`lookup(name)`**: "does this entry exist?" — called on `stat`, `ls <name>`, `cd <name>`
- **`children()`**: "list directory contents" — called on `readdir` / `ls <dir>/`

How the route tree resolves each:

- **Exact segments** (`"name"`) auto-emit a `VirtualNode::directory` during parent's children dispatch AND auto-match during parent's lookup dispatch. Both work automatically.
- **Captures** (`"{name}"`) match during tree walk only — they fire when the router recurses into sub-routes. A capture's children handler fires on readdir of the captured directory. **But captures alone don't make lookup succeed** — the parent node needs either a `lookup(handler)` or a children handler that returns matching directory nodes.
- **Glob** (`"**"`) with `lookup(handler)` fires as a fallback in `invoke_lookup` at any depth.

**Pattern: dynamic directories (search, query results)**

When any name should be a valid directory (e.g., `@/search/symbols/{query}`), combine a `lookup(handler)` on the parent with a `"{capture}" => children_handler`:

```rust
"symbols" {
    lookup(lookup_any_name),     // makes lookup("foo") succeed → returns directory
    "{query}" => children_results, // readdir on that directory → populates results
}
```

The lookup handler returns `Some(VirtualNode::directory(name))` for valid names (or `None` for ENOENT). The capture's children handler fires on subsequent readdir.

**Pattern: known set of directories (todo tags)**

When the valid names are known upfront, a children handler on the parent is sufficient — the dispatch system uses children results as lookup fallback:

```rust
"todo" => children_todo_root {   // returns [directory("TODO"), directory("FIXME")]
    "{tag}" => children_tag_dir, // readdir populates entries for matched tag
}
```

### Handler signatures

| Handler type | Signature |
|-|-|
| Children (`=>`) | `fn(&self, ctx: &RouteCtx<'_>) -> impl IntoNodes` |
| Lookup | `fn(&self, ctx: &RouteCtx<'_>, name: &str) -> impl IntoNode` |

Access captures via `ctx.param("name")` (single) or `ctx.params("name")` (rest). Import `RouteCtx` from `nyne::dispatch::routing::ctx`, `RouteTree` from `nyne::dispatch::routing::tree`.

## Source Staleness

Companion nodes are **automatically stamped** with source file and generation by `companion_children()`/`companion_lookup()`. Providers never call `VirtualNode::with_source()` manually. Dispatch uses the stamp for transparent stale-node re-resolution.

## Rules

- Provider structs are `pub(crate)` — nothing outside the plugin crate names a concrete provider type.
- `ProviderId` must be unique — `ProviderRegistry` panics on duplicates.
- No provider-specific code outside `providers/` — dispatch, FUSE, and CLI operate on `Arc<dyn Provider>` only.
- Store `Arc<ActivationContext>` directly — no `OnceLock`, no two-phase init.
- Access shared resources via TypeMap (`ctx.get::<Arc<MyService>>()`) — never duplicate services.
- For overlay conflicts, implement `on_conflict` returning `ConflictResolution::Force`.
