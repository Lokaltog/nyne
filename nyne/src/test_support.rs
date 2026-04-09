//! Shared test fixtures and stubs.
//!
//! Import with `use crate::test_support::*;` in any `tests.rs` module.
//! This is the single source of truth for common test helpers — never
//! duplicate these in individual test modules.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::{Result, bail};

use crate::config::NyneConfig;
use crate::dispatch::activation::ActivationContext;
use crate::process::Spawner;
use crate::router::{AffectedFiles, DirEntry, Filesystem, Metadata};

/// Stub [`Filesystem`] — all methods bail. Use when the test never touches
/// the filesystem (e.g., template rendering, snapshot assertions).
pub struct StubFs;

impl Filesystem for StubFs {
    fn source_dir(&self) -> &Path { Path::new("/stub") }

    fn read_dir(&self, _: &Path) -> Result<Vec<DirEntry>> { bail!("stub") }

    fn stat(&self, _: &Path, _: &str) -> Result<Option<DirEntry>> { bail!("stub") }

    fn read_file(&self, _: &Path) -> Result<Vec<u8>> { bail!("stub") }

    fn write_file(&self, _: &Path, _: &[u8]) -> Result<AffectedFiles> { bail!("stub") }

    fn rename(&self, _: &Path, _: &Path) -> Result<()> { bail!("stub") }

    fn remove(&self, _: &Path) -> Result<()> { bail!("stub") }

    fn create_file(&self, _: &Path) -> Result<()> { bail!("stub") }

    fn mkdir(&self, _: &Path) -> Result<()> { bail!("stub") }

    fn metadata(&self, _: &Path) -> Result<Metadata> { bail!("stub") }

    fn symlink_target(&self, _: &Path) -> Result<PathBuf> { bail!("stub") }
}

/// Load a test fixture file from `src/{module}/fixtures/{name}`.
///
/// Expands `env!("CARGO_MANIFEST_DIR")` at the call site so each crate
/// resolves fixtures relative to its own manifest root. Panics if the
/// file doesn't exist or can't be read — fixture absence is always a
/// test setup bug.
#[macro_export]
macro_rules! load_fixture {
    ($module:expr, $name:expr) => {{
        let __path = format!("{}/src/{}/fixtures/{}", env!("CARGO_MANIFEST_DIR"), $module, $name);
        ::std::fs::read_to_string(&__path).unwrap_or_else(|e| panic!("failed to read fixture {}: {}", __path, e))
    }};
}

/// Build a minimal [`ActivationContext`] for unit tests.
///
/// Uses [`StubFs`] and default config — suitable for tests that need a
/// context reference but don't exercise filesystem or config behavior.
pub fn stub_activation_context() -> ActivationContext {
    let tmp = PathBuf::from("/tmp/nyne-test");
    ActivationContext::new(
        tmp.clone(),
        tmp.clone(),
        tmp,
        Arc::new(StubFs),
        Arc::new(NyneConfig::default()),
        Arc::new(Spawner::new()),
    )
}
