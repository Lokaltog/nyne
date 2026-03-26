# Plugin Architecture

Two-phase lifecycle: `activate()` inserts services into `TypeMap`, then `providers()` creates provider instances reading from it. Registration via `#[linkme::distributed_slice(PLUGINS)]`, linkage via `use nyne_source as _;` in `main.rs`.

**Core ↛ plugin invariant:** nyne core must have ZERO knowledge of plugin concepts. No plugin-specific types, imports, or logic in `nyne/src/`. `TypeMap` is for plugins to talk to each other.

## Plugin Dependency Chain

```
nyne (core)
├── source          foundational — parsing, decomposition, editing
│   ├── lsp         progressive — LSP nodes in symbol directories
│   ├── analysis    progressive — HINTS.md in symbol directories
│   └── git         progressive — per-symbol blame/history
├── claude          consumer — hooks use source, lsp, analysis via TypeMap
└── todo            consumer — uses SyntaxRegistry for extension filtering
```

**Invariants:**
- **No cycles.** Dependencies flow downward only. Core → source → {lsp, analysis, git} → {claude, todo}.
- **Core ↛ plugins.** Core has zero knowledge of any plugin. `TypeMap` is the sole inter-plugin communication channel.
- **Source ↛ lsp/analysis/git.** Source defines extension traits (`FileRenameHook`) but has zero imports from downstream plugins. LSP/analysis/git implement these traits and insert them into TypeMap.
- **Progressive disclosure.** VFS nodes appear/disappear based on which plugins are loaded. Multiple providers contribute children to the same directory path — the dispatch layer auto-merges them.
- **Optional feature deps.** Downstream consumers (claude) gate plugin-specific imports with `#[cfg(feature = "...")]` and feature-flagged optional dependencies. Graceful degradation when a plugin is absent.

## Writing a New Plugin

1. Create `plugins/<name>/` with `Cargo.toml` (`nyne-plugin-<name>`, lib `nyne_<name>`), `src/lib.rs`, `src/plugin.rs`.
2. Register via `#[linkme::distributed_slice(PLUGINS)]` in `plugin.rs`.
3. `activate()` — insert services into TypeMap. Read services from other plugins via `ctx.get::<T>()` (returns `Option` — handle absence gracefully).
4. `providers()` — create `Provider` impls. Use `children()` to contribute nodes to existing directory paths (multi-provider composition).
5. Add workspace dependency alias in root `Cargo.toml` and `use nyne_<name> as _;` in `main.rs`.
6. Add `CLAUDE.md` documenting dependencies, TypeMap insertions/reads, and config section.
