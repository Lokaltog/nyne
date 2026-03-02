# Codebase Patterns

## Config Pattern

- `NyneConfig` — top-level config struct with `serde` + `garde` validation
- TOML deserialization with `deny_unknown_fields`
- Defaults via `impl Default`, loaded from XDG path (`~/.config/nyne/config.toml`) via `directories` crate
- Duration fields use `humantime-serde` for human-readable TOML (`"5m"`, `"2s"`)

## Enum Ergonomics

- Use `strum` derives for enum ↔ string: `Display`, `EnumString`, `EnumIter`, `EnumCount`
- Serde discriminators via `#[strum(serialize = "...")]`
- See `SymbolKind` for examples (`NodeKind` is now an opaque struct, not an enum)

## Event Draining (`process_events`)

Provider operations can emit invalidation events (e.g., cache busts, kernel dentry invalidations). These events accumulate in a `BufferedEventSink` and must be drained via `Router::process_events()`. Ownership of the drain call is split by category:

- **Mutations** (create, mkdir, remove, rename) — **dispatch owns it.** Each mutation method in `Router` calls `process_events()` internally before returning. The FUSE layer never drains events after calling a mutation method.
- **I/O and xattr** (flush, release, setxattr) — **FUSE owns it.** The write pipeline and xattr handlers don't drain events; the FUSE callback does after the operation completes.

This split exists because mutations already have complex internal cache management (inline eviction, sweep, invalidation) that naturally pairs with event draining, while I/O paths are simpler pass-throughs where the FUSE layer is the natural drain point.

**When adding new Router methods that may trigger provider events:** decide which category they belong to and follow the corresponding pattern. Don't drain from both layers for the same operation.

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

### When NOT to use Rope

- Simple `&str` operations that don't involve position math (contains, starts_with, split)
- Byte-range extraction where you already have the range (`&source[range]`)

### Pattern: LSP position conversion

```rust
use crop::Rope;

let rope = Rope::from(source_text);

// LSP Position → byte offset (O(log n))
let line_start = rope.byte_of_line(pos.line as usize);
let line_slice = rope.line(pos.line as usize);
let byte_offset = line_start + line_slice.byte_of_utf16_code_unit(pos.character as usize);

// byte offset → LSP Position (O(log n))
let line = rope.line_of_byte(offset);
let col = rope.byte_slice(rope.byte_of_line(line)..offset).utf16_len();
```

SSOT: all LSP position conversions live in `src/lsp/uri/mod.rs` (`position_to_byte_offset`, `byte_offset_to_position`). Don't duplicate — import from there.

### Pattern: multi-edit application

Build the Rope once per file, apply all edits in reverse position order. Each edit mutates the Rope in-place — subsequent offset lookups remain O(log n) even after mutations.

See `src/types/splice.rs` for the splice-validate-write pattern using Rope.
