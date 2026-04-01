// Test harness: setup failures and assertion failures panic by design —
// these ergonomic shortcuts are idiomatic for test code.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Integration test harness for nyne.
//!
//! Spawns a real `nyne mount` FUSE daemon on a temporary git repo clone,
//! executes test commands inside the mount namespace via `nyne attach`,
//! and asserts on captured output.
//!
//! Each test takes an owned [`NyneMount`] fixture — when the test returns,
//! `Drop` runs and the daemon is torn down. Mutating tests should use
//! [`NyneMount::cleanup_guard`] to restore the repo on drop, and annotate
//! themselves with `#[serial]`.
//!
//! # Example
//!
//! ```no_run
//! use nyne_integration_tests::{NyneMount, assert_ok, mount};
//! use rstest::rstest;
//!
//! #[rstest]
//! fn reads_git_status(mount: NyneMount) {
//!     let out = mount.sh("cat @/git/STATUS.md");
//!     assert_ok(&out);
//! }
//! ```

mod assertions;
mod command;
mod git;
mod harness;
pub mod targets;

pub use assertions::{assert_contains, assert_contains_any, assert_fails, assert_ok};
pub use command::CommandOutput;
pub use git::CleanupGuard;
pub use harness::NyneMount;

/// Per-test fixture: fresh `NyneMount` injected by value, so `Drop` runs
/// when the test returns. This is load-bearing — `#[once]` stores the
/// value in a static `OnceLock`, and the libtest/nextest harness exits
/// via `std::process::exit()` which does not run destructors on statics,
/// which orphaned the `nyne mount` daemon on every test run.
///
/// Under nextest (the configured runner) each test is a separate process,
/// so one mount per test is the same startup cost as a `#[once]` fixture.
///
/// Import and use in test files:
///
/// ```ignore
/// use nyne_integration_tests::{mount, NyneMount, assert_ok};
/// use rstest::rstest;
///
/// #[rstest]
/// fn my_test(mount: NyneMount) {
///     let out = mount.sh("cat @/git/STATUS.md");
///     assert_ok(&out);
/// }
/// ```
#[rstest::fixture]
#[allow(unused_braces)] // rstest's macro expansion triggers the lint on single-expr bodies
pub fn mount() -> NyneMount { NyneMount::start().expect("mount startup failed") }
