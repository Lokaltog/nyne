---
paths:
  - "nyne/**/*.rs"
  - "src/**/*.rs"
  - "plugins/**/*.rs"
---

# Codebase Patterns

## Config Pattern

- `NyneConfig` — top-level config struct with `serde` + `garde` validation
- TOML deserialization with `deny_unknown_fields`
- Defaults via `impl Default`, loaded from XDG path (`~/.config/nyne/config.toml`) via `directories` crate
- Duration fields use `humantime-serde` for human-readable TOML (`"5m"`, `"2s"`)
- Plugin configs use `#[serde(default, deny_unknown_fields)]` at struct level — `Default` impl is the SSOT for all default values (no per-field `#[serde(default = "fn")]`)
- Plugin configs implement `PluginConfig` trait (`Default + Serialize + Deserialize`) and wire into `Plugin` via `nyne::plugin_config!(Config)`
- Config merge operates on `toml::Value` throughout — no JSON intermediate

## Enum Ergonomics

- Use `strum` derives for enum ↔ string: `Display`, `EnumString`, `EnumIter`, `EnumCount`
- Serde discriminators via `#[strum(serialize = "...")]`
- See `SymbolKind` for examples (`NodeKind` is now an opaque struct, not an enum)

## Change Propagation (`Provider::on_change`)

When source files change, `fuse::notify::propagate_source_changes` is called with the affected paths. **This function is the single source of truth for change propagation** — both call sites below route through it. Divergence between them previously caused a regression where external modifications to `.git/**` (and any other non-companion path) left the kernel page cache stale. Do not reimplement the sequence in new call sites.

The sequence is:

1. Every provider's `on_change(&[PathBuf])` is called with the full batch. Providers bump their internal caches (e.g. `CacheProvider` generations) and may return `Vec<InvalidationEvent>` for *derived* virtual paths (e.g. companion `@/` namespaces).
2. **Every raw source path in the batch** is passed to `invalidate_inode_at`, which drops the kernel's page/dentry cache entry for that file. Skipping this step leaves FUSE serving stale content for externally-modified files until the attr TTL expires.
3. Every derived `InvalidationEvent` path is also passed to `invalidate_inode_at` so companion namespaces invalidate in lockstep with their source files.

Two call sites invoke `propagate_source_changes`:

- **Watcher path** (`watcher::EventLoop::flush`) — debounced filesystem events observed via `inotify`. Handles external mutations.
- **Inline write/mutation path** (`FuseFilesystem::notify_change`, called from `write_file` and `try_node_mutation`) — runs synchronously after a VFS write so caches invalidate before the next read (the watcher has a 50ms debounce that would otherwise leave a stale window).

`InvalidationEvent` is a simple struct with a single `path: PathBuf` field, defined in `router::provider`. Providers that only need to invalidate their own internal caches (e.g. `DecompositionCache`, LSP file state) do so directly in `on_change` and return an empty vec — the raw-path invalidation in step 2 handles kernel eviction regardless.

`CacheProvider::on_change` bumps both the changed path **and** its parent directory. The cache provider stores `Lookup`/`Readdir` entries with `source = req.path()` (the parent directory for `Lookup { name }`), matching the convention its own mutation branch uses (`Op::Create`/`Op::Remove` bump `source_from_request(req)`). External changes must honour the same convention or lookup cache entries go stale.

## Test Layout

Every module with tests uses the **directory module pattern** — the source file is `mod.rs` inside a directory, with tests as a child module:

```
src/config/
├── mod.rs           # source code, ends with: #[cfg(test)] mod tests;
├── tests.rs         # test module (use super::*;)
├── fixtures/        # test fixture files (TOML, J2, etc.)
└── snapshots/       # insta snapshot files (auto-generated)
```

- `#[cfg(test)] mod tests;` is always the **last item** in `mod.rs` — no `#[path]` attribute needed
- Test files use `use super::*;` for private access
- **Fixture files** live in `fixtures/` inside the module directory — loaded at runtime via `CARGO_MANIFEST_DIR`
- **Insta snapshots** live in `snapshots/` inside the module directory — committed to git, auto-generated on first run

See `doc/testing.md` for full testing conventions.

## Text Manipulation via `crop::Rope`

**Default tool for all text position/offset/line operations.** Never hand-roll character scanning, line counting, or UTF-16 offset arithmetic — `crop::Rope` does all of it in O(log n) with correct Unicode handling.

The `utf16-metric` feature is enabled, giving access to UTF-16 code unit metrics alongside byte and line metrics. This is critical for LSP interop (LSP positions use UTF-16 offsets).

### When to use Rope

- **Line ↔ byte offset conversion** — `byte_of_line(n)`, `line_of_byte(offset)`
- **UTF-16 ↔ byte offset conversion** — `byte_of_utf16_code_unit(n)`, `utf16_code_unit_of_byte(offset)`
- **Line counting** — `line_len()`
- **Slicing by any metric** — `byte_slice(range)`, `line_slice(range)`, `utf16_slice(range)`
- **Splice/replace operations** — `delete(range)`, `insert(offset, text)`, `replace(range, text)`
