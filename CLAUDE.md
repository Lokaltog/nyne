# nyne

FUSE overlay exposing agent-optimized access to decomposed source symbols, LSP intelligence, and git history as virtual files.

## Environment

- **Rust** edition 2024, `rust-version = "1.93"`, nightly toolchain
- **Build:** `cargo` workspace — root binary, `nyne/` (core lib), `nyne-macros/` (proc-macro), `plugins/`
- **Key crates:** `fuser` (FUSE), `color_eyre`/`thiserror` (errors), `minijinja` (templates), `linkme` (plugin registration)

**Pre-release / private.** All internal APIs can be freely changed.

## Workspace Crates

| Crate                | Path              | Role                                                                        |
| -------------------- | ----------------- | --------------------------------------------------------------------------- |
| `nyne-bin`           | `src/`               | Binary entry point, CLI dispatch                                            |
| `nyne`               | `nyne/`           | Core library — dispatch, FUSE, nodes, providers, config, templates, types   |
| `nyne-macros`        | `nyne-macros/`    | Proc macros (`routes!`)                                                     |
| `nyne-plugin-git`    | `plugins/git/`    | Git providers (blame/log/status/branches/diff/history), git-aware companion |
| `nyne-plugin-coding` | `plugins/coding/` | Syntax decomposition, LSP, batch edits, workspace search                    |
| `nyne-plugin-claude` | `plugins/claude/` | Claude Code integration — hooks, settings, skills, system prompt            |
| `nyne-plugin-todo`   | `plugins/todo/`   | TODO/FIXME comment aggregation and scanning                                 |

Plugin crates use `nyne-plugin-*` naming; root binary aliases them as `nyne-git`/`nyne-coding`/`nyne-claude`/`nyne-todo`.

## CLI Commands

| Command | Module | Purpose |
|-|-|-|
| `nyne mount` | `cli/mount.rs` | Start FUSE daemon(s) for directory(ies) |
| `nyne attach` | `cli/attach.rs` | Enter namespace of running mount, exec command |
| `nyne list` | `cli/list.rs` | Show sessions and attached processes |
| `nyne exec` | `cli/exec.rs` | Pipe-oriented script execution (binary stdin/stdout) |
| `nyne ctl` | `cli/ctl.rs` | Generic JSON control interface to a running daemon |
| `nyne config` | `cli/config.rs` | Dump resolved configuration |

`ctl` reads a `ControlRequest` JSON from arg or stdin, sends it to the daemon's control socket, and writes the `ControlResponse` as JSON to stdout. `ControlRequest` is the SSOT — no CLI-side type duplication.

## Plugin Architecture

Two-phase lifecycle: `activate()` inserts services into `TypeMap`, then `providers()` creates provider instances reading from it. Registration via `#[linkme::distributed_slice(PLUGINS)]`, linkage via `use nyne_git as _;` in `main.rs`.

**Core ↛ plugin invariant:** nyne core must have ZERO knowledge of plugin concepts. No plugin-specific types, imports, or logic in `nyne/src/`. `TypeMap` is for plugins to talk to each other.

## Protected Files

These require **explicit user confirmation** before modification: `Cargo.toml`, `deny.toml`, `rustfmt.toml`, `rust-toolchain.toml`.

**Adding dependencies:** versions centralized in `[workspace.dependencies]` in root `Cargo.toml`. Use `cargo add` to find latest version, add there first, then `dep.workspace = true` in member crates.

## Verification

Before committing, run `just check`. This runs fmt, clippy, deny, check, and nextest in sequence. Avoid running these commands individually.

## Documentation

<required-reading>
Read the docs relevant to your task before starting work:

- **Any code task** → `doc/conventions.md` (mandatory)
- Modifying source code → `doc/codebase.md`
- Changing interfaces or moving code → `doc/refactoring.md`
- Writing or modifying tests → `doc/testing.md`
</required-reading>
