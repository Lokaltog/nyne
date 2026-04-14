# nyne (core library)

Core library — all plugin crates depend on this. Module structure is discoverable via VFS OVERVIEW.md files.

## Module Tiers

Modules may only import from their own tier or lower.

- **Tier 0 — Foundation**: `types/`, `text/`, `config/`, `session/`, `process/`, `procfs/`, `deep_merge/`, `path_utils`, `path_filter/`
- **Tier 1 — Infrastructure**: `router/`
- **Tier 2 — Domain**: `err/`
- **Tier 3 — Contracts** (imports lower tiers): `plugin/`, `prelude/`, `templates/`
- **Tier 4 — Orchestration** (imports any lower tier): `dispatch/`, `fuse/`, `watcher/`, `sandbox/`, `cli/`

Within-tier imports are allowed and encouraged when they enable code sharing — the constraint is strictly on tier ordering, not isolation.

## Core ↛ Plugin Invariant

nyne core must have ZERO knowledge of plugin concepts. No plugin-specific types, imports, or logic in `nyne/src/`. The request state map is for plugins to talk to each other.

All state types must impl `Clone + Send + Sync` (required for cache state snapshotting).
