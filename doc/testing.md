# Testing

Testing philosophy and practices. Load this when writing or modifying tests.

## TDD Workflow

**TDD is the preferred workflow.** RED-GREEN-REFACTOR:

1. **RED** — Write a failing test that defines the expected behavior.
2. **GREEN** — Write the minimum code to make it pass.
3. **REFACTOR** — Clean up while keeping tests green.

## Core Principles

- **Bug fixes require a reproducing test.** Add a test that fails before the fix and passes after. This is mandatory — a bug fix without a test is incomplete.
- **Tests prove the change works.** A feature or fix without a corresponding test is not done.
- **Never widen production APIs to accommodate tests.** If a mock isn't reached because an intermediate call fails, fix the test setup (mock the intermediate), don't change the production signature.

## Test Infrastructure

- **Runner:** `cargo test`
- **Parameterized tests:** `rstest` — use `#[rstest]` with `#[case]` for parameterized tests, `#[fixture]` for shared setup. Prefer over duplicated `#[test]` functions with identical structure.
- **Property-based tests:** `proptest` — use `proptest!` macro for invariant testing (round-trips, range consistency, parser properties). Place in a `proptest` submodule within `tests.rs`.
- **Snapshot testing:** `insta` — use `insta::assert_snapshot!`, `assert_yaml_snapshot!`, `assert_debug_snapshot!`. Redact volatile fields (UUIDs, timestamps) with `{ ".field" => "[redacted]" }`. Prefer snapshots over multi-line `assert_eq!` chains on structs.
- **CLI integration tests:** `assert_cmd` + `predicates` for binary testing. Use `Command::cargo_bin("nyne")` to build and run the binary.
- **Assertions:** `similar-asserts::assert_eq!` for better diffs on complex types. Use standard `assert!`/`assert_eq!` for simple checks.
- **Temp files:** `tempfile` + `assert_fs` for filesystem test fixtures.
- **Shared test helpers:** `test_support` module (`src/test_support.rs`, `#[cfg(test)]`) — shared fixtures and stubs. Import with `use crate::test_support::*;` instead of duplicating helpers across test files.

## Conventions

### Module layout

Every module with tests uses the **directory module pattern**. The source file becomes `mod.rs` inside a directory, and tests live as a `tests.rs` child module:

```
src/config/
├── mod.rs           # source code, ends with: #[cfg(test)] mod tests;
├── tests.rs         # test module (use super::*;)
├── fixtures/        # test fixture files (TOML, J2, etc.)
└── snapshots/       # insta snapshot files (auto-generated)
```

- **Declaration:** `#[cfg(test)] mod tests;` — always the **last item** in `mod.rs`. No `#[path]` needed.
- **Test files** use `use super::*;` — they are child modules with full private access.
- **Snapshots** are auto-placed in `snapshots/` relative to the test file. Commit `.snap` files. Accept with `INSTA_UPDATE=always cargo test`.

### Converting a flat module

If `foo.rs` needs tests, convert it to a directory module first:

```sh
mkdir -p src/foo
git mv src/foo.rs src/foo/mod.rs
```

Then add `tests.rs` in the new directory.

### General

- **Prefer `#[rstest]` with `#[case]`** when 2+ tests share the same structure but differ in inputs/expected outputs. This is the default choice — reach for it before writing individual `#[test]` functions.
- Use `#[test]` directly only for truly one-off tests with unique structure.
- Use `#[fixture]` for shared setup that multiple tests need (e.g., `SyntaxRegistry`, `TemplateEngine`).
- Use `proptest!` for invariant properties (round-trips, non-overlapping ranges, consistency checks).

## Test Fixtures

- **Fixture files** live in `fixtures/` inside the module directory (e.g., `src/config/fixtures/*.toml`, `src/templates/fixtures/*.j2`).
- Load fixtures at runtime via `CARGO_MANIFEST_DIR`:

  ```rust
  fn load_fixture(name: &str) -> NyneConfig {
      let path = format!("{}/src/config/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
      let content = std::fs::read_to_string(&path)
          .unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"));
      toml::from_str(&content)
          .unwrap_or_else(|e| panic!("failed to parse fixture {name}: {e}"))
  }
  ```

- **When to use fixtures vs inline:** Use fixture files for valid inputs that test deserialization/rendering (editor-friendly, syntax-highlighted, visible in diffs). Keep inline strings for error-case tests where the input is intentionally malformed — seeing the bad input at the assertion site is clearer.
- **When to use snapshots vs assertions:** Use `insta` snapshots for structural assertions on deserialized structs or rendered output (catches regressions from field additions/removals). Use explicit assertions for `HashMap`-keyed data (non-deterministic key ordering makes snapshots flaky) and for targeted field checks.
