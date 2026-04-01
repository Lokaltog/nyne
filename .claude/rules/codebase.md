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

When source files change (filesystem watcher debounce, or inline after a VFS write/mutation), each provider's `on_change(&[PathBuf])` is called with the affected source paths. Providers return `Vec<InvalidationEvent>` for *derived* virtual paths that are now stale (e.g., companion `@/` namespace paths). The caller iterates the returned events and calls `invalidate_inode_at` to evict kernel page/dentry caches.

Two call sites drive this:

- **Watcher path** (`watcher::flush`) — debounced filesystem events. Calls `provider.on_change(&paths)` for every provider, then `invalidate_inode_at` for each returned event.
- **Inline write/mutation path** (`FuseFilesystem::write_file`, `try_node_mutation`) — called synchronously after a VFS write so caches invalidate before the next read (the watcher has a 50ms debounce that would leave a stale window).

`InvalidationEvent` is a simple struct with a single `path: PathBuf` field, defined in `router::provider`. Providers that only need to invalidate their own internal caches (e.g., `DecompositionCache`, LSP file state) do so directly in `on_change` and return an empty vec.

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
