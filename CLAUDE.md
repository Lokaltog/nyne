# nyne

FUSE virtual filesystem exposing agent-optimized access to decomposed source symbols, LSP intelligence, and git history as virtual files.

## Environment

- **Rust** edition 2024, `rust-version = "1.93"`, nightly toolchain
- **Build:** `cargo` workspace — root binary, `nyne/` (core lib), `plugins/`
- **Key crates:** `fuser` (FUSE), `color_eyre`/`thiserror` (errors), `minijinja` (templates), `linkme` (plugin registration)


## Workspace Crates

| Crate | Path | Alias | Role |
|-|-|-|-|
| `nyne-bin` | `src/` | — | Binary entry point, CLI dispatch |
| `nyne` | `nyne/` | — | Core library — router, FUSE bridge, config, templates, dispatch, types |
| `nyne-plugin-companion` | `plugins/companion/` | `nyne-companion` | Companion path-rewriting middleware (`@` suffix) |
| `nyne-plugin-cache` | `plugins/cache/` | `nyne-cache` | Resolution cache middleware |
| `nyne-plugin-slice` | `plugins/slice/` | `nyne-slice` | Line-range slicing middleware (`:{spec}`) |
| `nyne-plugin-diff` | `plugins/diff/` | `nyne-diff` | Diff preview-and-apply middleware (`.diff` files) |
| `nyne-plugin-visibility` | `plugins/visibility/` | `nyne-visibility` | Per-process visibility middleware |
| `nyne-plugin-fs` | `plugins/fs/` | `nyne-fs` | Terminal filesystem provider |
| `nyne-plugin-source` | `plugins/source/` | `nyne-source` | Tree-sitter parsing, decomposition, splice engine, batch edits |
| `nyne-plugin-lsp` | `plugins/lsp/` | `nyne-lsp` | LSP infrastructure, workspace search, LSP-powered VFS nodes |
| `nyne-plugin-analysis` | `plugins/analysis/` | `nyne-analysis` | Static analysis engine, code smell rules, HINTS.md |
| `nyne-plugin-git` | `plugins/git/` | `nyne-git` | Git providers (blame/log/status/branches/diff/history) |
| `nyne-plugin-claude` | `plugins/claude/` | `nyne-claude` | Claude Code integration — hooks, settings, skills, system prompt |
| `nyne-plugin-nyne` | `plugins/nyne/` | `nyne-nyne` | Mount meta-information (`.nyne.md` status file) |
| `nyne-plugin-todo` | `plugins/todo/` | `nyne-todo` | TODO/FIXME comment aggregation and scanning |
| `nyne-integration-tests` | `tests/integration/` | — | E2E integration tests — spawns real FUSE mount, asserts via `nyne attach` |

Plugin crates use `nyne-plugin-*` naming; root binary aliases them as `nyne-source`/`nyne-lsp` etc. All providers are opt-in and composed at startup.

Integration tests live in their own crate to depend on the `nyne` binary (spawned as subprocess via `nyne mount` and `nyne attach`). Run with `just integration`. See `tests/integration/CLAUDE.md`.

## Protected Files

These require **explicit user confirmation** before modification: `Cargo.toml`, `deny.toml`, `rustfmt.toml`, `rust-toolchain.toml`.

**Adding dependencies:** versions centralized in `[workspace.dependencies]` in root `Cargo.toml`. Use `cargo add` to find latest version, add there first, then `dep.workspace = true` in member crates.

## Verification

Before committing, run `just check`. This runs fmt, clippy, check, and nextest in sequence. Avoid running these commands individually.
