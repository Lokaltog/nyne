# Providers

Providers implement the `Provider` trait and produce VFS content. Core providers (this directory) are registered by dispatch; plugin providers live in `plugins/` and are discovered at link time.

For template rendering patterns and `HandleBuilder`/`TemplateHandle` workflow, see `doc/codebase.md`.

## Return Type Aliases

- **`Nodes`** = `Result<Option<Vec<VirtualNode>>>` — used by `children()`
- **`Node`** = `Result<Option<VirtualNode>>` — used by `lookup()`, `create()`, `mkdir()`

Route handler methods can return simpler types (`Vec<VirtualNode>`, `Option<VirtualNode>`) — `IntoNodes`/`IntoNode` traits wrap them automatically.

## Route Sub-directory Visibility

Exact sub-routes auto-emit a `VirtualNode::directory(name)` in the parent's readdir. Use `no_emit` to suppress for lookup-only routes:

```rust
no_emit "@" => children_at_root,  // hidden -- lookup-only
```

Captures (`{name}`), rest-captures (`{..path}`), and globs (`**`) never auto-emit.

## routes! Macro Syntax

Segments without handlers use bare blocks: `"name" { ... }`. The `=>` arrow is only for attaching a handler: `"name" => handler_fn { ... }`. Captures: `"{param}" => handler_fn,`.

## Source Staleness

Companion nodes are **automatically stamped** with source file and generation by `companion_children()`/`companion_lookup()`. Providers never call `VirtualNode::with_source()` manually. Dispatch uses the stamp for transparent stale-node re-resolution.

## Rules

- Provider structs are `pub(crate)` — nothing outside the plugin crate names a concrete provider type.
- `ProviderId` must be unique — `ProviderRegistry` panics on duplicates.
- No provider-specific code outside `providers/` — dispatch, FUSE, and CLI operate on `Arc<dyn Provider>` only.
- Store `Arc<ActivationContext>` directly — no `OnceLock`, no two-phase init.
- Access shared resources via TypeMap (`ctx.get::<Arc<MyService>>()`) — never duplicate services.
- For overlay conflicts, implement `on_conflict` returning `ConflictResolution::Force`.
