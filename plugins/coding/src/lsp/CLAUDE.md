# LSP Registration and Access

## Registration pattern (`register_lsp!`)

Mirrors the syntax and provider patterns — `linkme` distributed slice, one trait impl per language:

```rust
// src/lsp/languages/go.rs
use super::prelude::*;

struct GoLsp;

impl LspSpec for GoLsp {
    const EXTENSIONS: &'static [&'static str] = &["go"];

    fn servers() -> Vec<LspServerDef> {
        vec![LspServerDef::new("gopls")
            .detect(|root| root.join("go.mod").exists())]
    }
}

register_lsp!(GoLsp);
```

Two files touched: the language file + `mod go;` in `src/lsp/languages/mod.rs`. No other changes.

## LSP server spawning

LSP servers run as **direct children of the daemon process**, spawned via `Spawner` (in `src/process/`). The `Spawner` wraps `Command::new().spawn()` with `env_clear()` + explicit env propagation, captures stdio as `OwnedFd`s, and owns `Child` handles for lifecycle management. No cross-namespace protocol — FUSE re-entrancy is avoided because the daemon uses the storage root (overlay merged path or passthrough bind mount) for all I/O, which is a separate mount point from FUSE.

**Eager spawning:** All applicable LSP servers are spawned on a background thread (`lsp-eager-spawn`) during plugin activation via `LspManager::spawn_all_applicable()`. This ensures servers are warm by the time workspace-wide queries (e.g. symbol search) arrive, without blocking mount startup. Individual `client_for_ext` calls still work as a lazy fallback for extensions added after activation.

**Path contract:** `LspHandle.lsp_file` uses `ctx.overlay_root()`. LSP servers see the storage root path.

## Layering constraint

`lsp/` → imports `syntax/`, `types/`, `config/`, `process/`. **`syntax/` never imports `lsp/`** — the dependency is strictly one-directional.
