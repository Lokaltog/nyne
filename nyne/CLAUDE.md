# nyne (core library)

Core library — all plugin crates depend on this. Module structure is discoverable via VFS OVERVIEW.md files.

## Module Tiers

Modules may only import from their own tier or lower.

- **Tier 0 — Foundation** (no crate imports): `types/`, `format/`, `config/`, `session/`, `process/`
- **Tier 1 — Domain Knowledge** (imports Tier 0 + dispatch interface types†): `node/`, `edit/`
- **Tier 2 — Contracts & Infrastructure** (imports Tiers 0-1): `provider/`, `templates/`
- **Tier 3 — Orchestration** (imports any lower tier): `dispatch/`, `fuse/`, `watcher/`, `sandbox/`, `providers/`, `cli/`

† `ActivationContext` and `RequestContext` are interface types used in trait signatures across all tiers.

## dispatch/ — Interface vs Implementation

**Interface submodules** (imported freely): `activation`, `context`, `invalidation`, `resolver`, `script`, `write_mode`, `routing/`

**Implementation submodules** (dispatch-internal only): `router/`, `cache/`, `content_cache`, `pipeline`, `mutation`, `resolve`, `inode`, `events`, `path_filter`, `registry`, `script_registry`

## Submodule Access

Prefer re-exports from `mod.rs`. Reaching into implementation submodules of another module is a layering violation.

## CLI Output

All CLI terminal output goes through `cli::output` — the SSOT for terminal access. Import `Term`, `style`, and `term()` from `super::output`, never use `println!` or import `console::` directly in CLI modules.
