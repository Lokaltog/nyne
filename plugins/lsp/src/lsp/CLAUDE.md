# LSP Registration and Access

## Config-driven server definitions

LSP servers are defined declaratively in config, not source code. Built-in defaults live in `config/lsp.rs::default_servers()` using the `server!` macro. Users override or extend via `[plugin.coding.lsp.servers]` in `config.toml`.

Adding a new built-in server: one `server!(...)` entry in `default_servers()`. No other changes.

Adding a server via config (no recompilation):

```toml
[[plugin.coding.lsp.servers]]
name = "gopls"
command = "gopls"
extensions = ["go"]
language_ids = "go"
root_markers = ["go.mod"]
```

Overriding an existing server:

```toml
[[plugin.coding.lsp.servers]]
name = "rust-analyzer"
args = ["--log-file", "/tmp/ra.log"]
```

Disabling a built-in:

```toml
[[plugin.coding.lsp.servers]]
name = "basedpyright"
enabled = false
```

## Multi-tier config merge

Config layers merge in priority order: plugin defaults → user config → project config. Arrays are concatenated across layers. For servers, `LspRegistry::resolve_servers()` deduplicates by name (later entries override earlier ones per-field via `ServerEntry::overlay()`).

## LSP server spawning

LSP servers run as **direct children of the daemon process**, spawned via `Spawner` (in `src/process/`). The `Spawner` wraps `Command::new().spawn()` with `env_clear()` + explicit env propagation, captures stdio as `OwnedFd`s, and owns `Child` handles for lifecycle management. No cross-namespace protocol — FUSE re-entrancy is avoided because the daemon uses the storage root (overlay merged path or passthrough bind mount) for all I/O, which is a separate mount point from FUSE.

**Eager spawning:** All applicable LSP servers are spawned on a background thread (`lsp-eager-spawn`) during plugin activation via `LspManager::spawn_all_applicable()`. This ensures servers are warm by the time workspace-wide queries (e.g. symbol search) arrive, without blocking mount startup. Individual `client_for_ext` calls still work as a lazy fallback for extensions added after activation.

**Path contract:** `LspHandle.lsp_file` uses `ctx.overlay_root()`. LSP servers see the storage root path.

## Layering constraint

`lsp/` → imports `syntax/`, `types/`, `config/`, `process/`. **`syntax/` never imports `lsp/`** — the dependency is strictly one-directional.
