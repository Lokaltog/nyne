# Rust Conventions

## Error Handling

Two complementary tools — `thiserror` for structured errors, `eyre` for ergonomic context propagation. They coexist by design: `thiserror` types implement `std::error::Error` and auto-convert to `eyre::Report` via `?`.

- **Application boundary** (main, cli, fuse): use `color_eyre::eyre::Result` with `.wrap_err()` for context
- **Module error types**: use `thiserror` when callers need to distinguish error variants (match on them, retry, take different paths). Scope to `pub(crate)` or narrower
- **Internal helpers** where no caller matches on error kind: `eyre::Result` is fine — don't over-type errors nobody inspects
- Fail fast — don't swallow errors or add speculative fallbacks
- Validate at system boundaries only (user input, external APIs); trust internal code

## Logging

- Use `tracing` macros exclusively: `trace!`, `debug!`, `info!`, `warn!`, `error!`
- Never use `println!` for debug output — only for intentional user-facing terminal output
- Never use `eprintln!` for logging — use tracing with `std::io::stderr` writer
- Use structured fields: `info!(session_id = sid, "message")` not `info!("session_id={sid}")`
- Use targets for subsystem filtering: `trace!(target: "wire", ...)` enables `RUST_LOG=wire=trace`

## Naming

- Types: `PascalCase`
- Functions/methods: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`
- Enum variants: `PascalCase`
- Modules: `snake_case`
- Avoid module name repetition in type names (e.g., `control::Request` not `control::ControlRequest`)

## Code Style

- Edition 2024: `ref` in match patterns is implicit when matching on `&T` — don't use explicit `ref`
- Prefer `?` operator over `.unwrap()` in fallible paths
- Use `if let` / `let else` over `match` for single-pattern checks
- Prefer iterators over manual loops
- Prefer derive macros (`thiserror`, `strum`) over manual trait impls where available
- Keep functions focused — extract when a function does more than one logical thing
- **Early returns over nesting.** Use early `return` / `continue` / `let else` to flatten control flow — never nest if→if→if when guard clauses work. In closures, use `return` to avoid deep else branches.

## Serde Conventions

- Use `deny_unknown_fields` on config structs to catch typos early
- Use `#[serde(default)]` for optional collection fields (e.g., `Vec<T>`)
- Use `#[serde(skip_serializing)]` for secrets — never serialize API keys
- Avoid `#[serde(tag)] + #[serde(flatten)]` combinations — fragile, known serde edge cases

## Rules

1. **Never use `println!`/`eprintln!`** — use `tracing` macros for logging; use `cli::output` (`Term::write_line`, `style`) for user-facing terminal output
2. **Never expose `eyre::Report` from module public APIs** — use `thiserror` for errors callers need to match on; `eyre` for application-level context
3. **Never use `log` crate** — use `tracing` (provides `log` compatibility layer)
4. **Always use `serde` derive feature** — never manual Serialize/Deserialize impls unless required
5. **Prefer `std::sync::LazyLock`** over `once_cell` (stable since Rust 1.80)
6. **Use `just check`** as the standard test runner
7. **No ASCII art separators** in code comments — doc comments on functions are sufficient
8. **Never shell out to external commands** (`std::process::Command`) without explicit user pre-approval — use library APIs instead
