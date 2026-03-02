# nyne

FUSE virtual filesystem exposing agent-optimized access to decomposed source code.

**Pre-release / private.** All internal APIs can be freely changed.

## Environment

- **Rust** edition 2024, `rust-version = "1.93"`, nightly toolchain
- **Build:** `cargo` workspace — root binary, `nyne/` (core lib), `nyne-macros/` (proc-macro), `plugins/`
- **Key crates:** `fuser` (FUSE), `color_eyre`/`thiserror` (errors), `minijinja` (templates), `linkme` (plugin registration)

## Workspace Crates

| Crate                | Path              | Role                                                                        |
| -------------------- | ----------------- | --------------------------------------------------------------------------- |
| `nyne-bin`           | `/`               | Binary entry point, CLI dispatch                                            |
| `nyne`               | `nyne/`           | Core library — dispatch, FUSE, nodes, providers, config, templates, types   |
| `nyne-macros`        | `nyne-macros/`    | Proc macros (`routes!`)                                                     |
| `nyne-plugin-git`    | `plugins/git/`    | Git providers (blame/log/status/branches/diff/history), git-aware companion |
| `nyne-plugin-coding` | `plugins/coding/` | Syntax decomposition, LSP, claude integration, batch edits, todo tracking   |

Plugin crates use `nyne-plugin-*` naming; root binary aliases them as `nyne-git`/`nyne-coding`.

## Plugin Architecture

Two-phase lifecycle: `activate()` inserts services into `TypeMap`, then `providers()` creates provider instances reading from it. Registration via `#[linkme::distributed_slice(PLUGINS)]`, linkage via `use nyne_git as _;` in `main.rs`.

**Core ↛ plugin invariant:** nyne core must have ZERO knowledge of plugin concepts. No plugin-specific types, imports, or logic in `nyne/src/`. `TypeMap` is for plugins to talk to each other.

## SSOT / DRY

**If a change requires edits in multiple places to stay consistent, there is a missing abstraction.** Stop. Extract the shared knowledge into one authoritative location before continuing.

- Duplication you believe is justified → stop and ask for explicit approval.
- Existing violation you encounter → fix it now, or ask if the fix would derail your task.

### Recognizing Violations

- Parallel match arms on the same enum in different files
- Copy-pasted structs with slight variations
- Format strings or path templates repeated in multiple locations
- Adding a variant/field requires updating more than one file
- Constants or magic values as literals in more than one place

## Visibility

- **Default to private.** `pub(super)` > `pub(crate)` > `pub`.
- **`pub` means external API.** Never widen visibility to fix a compile error — reconsider the dependency direction.

## Protected Files

These require **explicit user confirmation** before modification: `Cargo.toml`, `deny.toml`, `rustfmt.toml`, `rust-toolchain.toml`.

**Adding dependencies:** versions centralized in `[workspace.dependencies]` in root `Cargo.toml`. Use `cargo add` to find latest version, add there first, then `dep.workspace = true` in member crates.

## Following Conventions

**Documented conventions are mandatory rules, not guidelines.** When a doc says "every module with tests uses X" or "always do Y", there are zero exceptions unless the user explicitly grants one.

Specific failure modes to avoid:

- **Do not self-assess whether a convention "applies" to your situation.** It does. If the doc says "every", it means every — not "every, unless I think the file is small."
- **Do not inflate effort to justify skipping a convention.** Converting `foo.rs` to `foo/mod.rs` is two commands (`mkdir + git mv`), not a "larger refactoring." Mechanical restructuring is never a reason to deviate.
- **Do not invent exceptions that don't exist in the text.** If the convention has no size qualifier, scope qualifier, or escape hatch, then none exists.
- **When in doubt, follow the convention literally.** If you believe a convention genuinely shouldn't apply, **stop and ask the user** — never silently substitute your own judgment.
- **Do not implement without an approved plan.** When a task involves changing the codebase, present your proposed changes first — what you intend to modify, where, and why — then wait for explicit approval before editing any file. Once the user approves, execute fully: edit, verify, commit. "Analyze this" or "propose a fix" is not approval to implement.

## Verification

**Before committing, always run `just check`.** This runs fmt, clippy, deny, check, and nextest in sequence. Never run these individually.

## Documentation

<required-reading>
Read the docs relevant to your task before starting work:

- **Any code task** → `doc/conventions.md` (mandatory)
- Modifying source code → `doc/codebase.md`
- Changing interfaces or moving code → `doc/refactoring.md`
- Writing or modifying tests → `doc/testing.md`
- Committing changes → `doc/commits.md`

Don't explore the codebase to discover patterns that are already documented here.
</required-reading>
