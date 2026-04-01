# nyne-integration-tests

E2E test harness. Spawns a real `nyne mount` daemon with `--storage-strategy snapshot` (libgit2 ODB clone) and asserts VFS behavior via `nyne attach`.

## Running

```sh
just integration                         # all tests
just integration -- t_003                # filter by name
NYNE_BIN=/path/to/nyne just integration  # specific binary
```

Must run outside the sandbox (daemon needs host filesystem access for libgit2 snapshot cloning).

## Architecture

- `harness.rs` — `NyneMount` RAII fixture; spawns daemon with `PR_SET_PDEATHSIG(SIGTERM)`, polls readiness, sends SIGTERM on drop.
- `command.rs` — `CommandOutput` captures stdout/stderr/exit code.
- `git.rs` — `CleanupGuard` runs `git checkout HEAD -- .` on drop.
- `assertions.rs` — `assert_ok`, `assert_fails`, `assert_contains[_any]`.
- `targets.rs` — shared test target constants (`targets::rust`, `targets::lsp`). Use these instead of inlining paths.

## Writing Tests

Each test takes an owned `NyneMount` via rstest. The fixture is NOT `#[once]` — static fixtures aren't dropped when nextest exits via `process::exit()`, orphaning daemons.

```rust
#[rstest]
fn my_test(mount: NyneMount) {
    let out = mount.sh("cat @/git/STATUS.md");
    assert_ok(&out);
}
```

Mutating tests must use `#[serial_test::serial]` and declare `let _guard = mount.cleanup_guard();` first.

Test targets: import from `targets::rust::{FILE, SYMBOL}` instead of hardcoding paths.

## Isolation

Each `NyneMount::start` generates a unique session ID (`test-<pid>-<counter>`) and spawns an independent daemon.

## Environment Variables

| Variable | Purpose |
|-|-|
| `NYNE_BIN` | Path to `nyne` binary (default: `$CARGO_TARGET_DIR/<profile>/nyne`) |
| `CARGO_TARGET_DIR` | Override target directory for binary lookup |
