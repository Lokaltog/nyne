# LSP Session

## Config-driven server definitions

Built-in defaults in `plugin/config/mod.rs::default_servers()` using `server!` macro. Adding a server: one `server!(...)` entry. User overrides via `[plugin.lsp.servers.<name>]` TOML — deep merge handles per-key deduplication.

## LSP server spawning

Servers run as daemon children via `Spawner` (nyne core `process/`). Eager spawning during activation for warm workspace queries; `client_for_ext` is lazy fallback for late extensions.

**Path contract:** `Handle::for_file` reads `source_root` from `Manager.path_resolver()`. LSP servers see the storage root path.
