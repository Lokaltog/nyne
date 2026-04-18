# LSP Session

## Config-driven server definitions

Built-in defaults in `plugin/config/mod.rs::default_servers()` using `server!` macro. Adding a server: one `server!(...)` entry. User overrides via `[plugin.lsp.servers.<name>]` TOML — deep merge handles per-key deduplication.

## LSP server spawning

Servers run as daemon children via `Spawner` (nyne core `process/`). Eager spawning during activation for warm workspace queries; `client_for_ext` is lazy fallback for late extensions.

**Path contract:** `Handle::for_file` reads `source_root` from `Manager.path_resolver()`. LSP servers see the storage root path.

## Client lifecycle and progress gating

`Client::spawn` order is significant:

1. Spawn writer/reader threads (reader owns the `ProgressTracker` and
   dispatches `$/progress` to it).
2. Send `initialize` -> wait for response -> send `initialized`. This
   runs through `send_request`, which is safe because the tracker
   starts in `Uninitialized` and `wait_ready` returns immediately.
3. `progress.arm()` -- transitions `Uninitialized -> Indexing`,
   carrying any tokens that arrived during step 2.

After `arm`, every `send_request` call parks on `wait_ready(index_timeout)`
until indexing quiesces (or the grace timeout expires and force-readies
the tracker). `send_notification` does not park -- `didOpen`/`didChange`
must flow during indexing so the server has file contents to index.

`Client::shutdown` (called from `Drop_for_Client`) calls
`progress.shutdown()` before issuing the LSP `shutdown` request. The
terminal `Shutdown` state releases all parked waiters with a `ready`
signal so the shutdown request itself does not block on the gate.

See `plugins/lsp/CLAUDE.md` for the state-machine rationale.
