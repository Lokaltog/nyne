//! Shared test fixtures and stubs.
//!
//! Import with `use crate::test_support::*;` in any `tests.rs` module.
//! This is the single source of truth for common test helpers — never
//! duplicate these in individual test modules.

use std::path::Path;
use std::sync::Arc;

use color_eyre::eyre::{Result, bail};

use crate::dispatch::content_cache::FileGenerations;
use crate::dispatch::context::RequestContext;
use crate::dispatch::invalidation::{EventSink, InvalidationEvent};
use crate::dispatch::resolver::Resolver;
use crate::node::VirtualNode;
use crate::types::real_fs::{DirEntry, FileMeta, RealFs};
use crate::types::vfs_path::VfsPath;

// Shared constructors

/// Shorthand: parse a string into a `VfsPath` (panics on invalid input).
pub fn vfs(s: &str) -> VfsPath { VfsPath::new(s).unwrap() }

// Stub trait implementations

/// Stub `RealFs` — all methods bail. Use when the test never touches the
/// real filesystem (e.g., template rendering, snapshot assertions).
pub struct StubFs;

impl RealFs for StubFs {
    fn source_dir(&self) -> &Path { Path::new("/stub") }

    fn read(&self, _: &VfsPath) -> Result<Vec<u8>> { bail!("stub") }

    fn write(&self, _: &VfsPath, _: &[u8]) -> Result<()> { bail!("stub") }

    fn exists(&self, _: &VfsPath) -> bool { false }

    fn is_dir(&self, _: &VfsPath) -> bool { false }

    fn read_dir(&self, _: &VfsPath) -> Result<Vec<DirEntry>> { bail!("stub") }

    fn metadata(&self, _: &VfsPath) -> Result<FileMeta> { bail!("stub") }

    fn symlink_target(&self, _: &VfsPath) -> Result<std::path::PathBuf> { bail!("stub") }

    fn rename(&self, _: &VfsPath, _: &VfsPath) -> Result<()> { bail!("stub") }

    fn unlink(&self, _: &VfsPath) -> Result<()> { bail!("stub") }

    fn rmdir(&self, _: &VfsPath) -> Result<()> { bail!("stub") }

    fn create_file(&self, _: &VfsPath) -> Result<()> { bail!("stub") }

    fn mkdir(&self, _: &VfsPath) -> Result<()> { bail!("stub") }
}

/// Stub `EventSink` — silently discards all events.
pub struct StubEvents;

impl EventSink for StubEvents {
    fn emit(&self, _: InvalidationEvent) {}
}

/// Stub `Resolver` — all lookups bail.
pub struct StubResolver;

impl Resolver for StubResolver {
    fn resolve(&self, _: &VfsPath) -> Result<Vec<Arc<VirtualNode>>> { bail!("stub") }

    fn lookup(&self, _: &VfsPath) -> Result<Option<Arc<VirtualNode>>> { bail!("stub") }
}

/// Build a `RequestContext` wired to stubs. Useful when the test exercises
/// a `Readable` impl that doesn't actually touch the context.
pub fn stub_request_context<'a>(
    path: &'a VfsPath,
    real_fs: &'a StubFs,
    events: &'a StubEvents,
    resolver: &'a StubResolver,
    file_generations: &'a FileGenerations,
) -> RequestContext<'a> {
    RequestContext {
        path,
        real_fs,
        events,
        resolver,
        file_generations,
    }
}

/// Convenience bundle for tests that need a `RequestContext` at a specific path.
///
/// Owns all stubs so the test only needs to keep this struct alive.
pub struct StubRequestContext {
    pub path: VfsPath,
    pub real_fs: StubFs,
    pub events: StubEvents,
    pub resolver: StubResolver,
    pub file_generations: FileGenerations,
}

impl StubRequestContext {
    /// Borrow a `RequestContext` from the owned stubs.
    pub fn ctx(&self) -> RequestContext<'_> {
        stub_request_context(
            &self.path,
            &self.real_fs,
            &self.events,
            &self.resolver,
            &self.file_generations,
        )
    }
}

/// Build a stub context bundle for a given VFS path string.
///
/// # Panics
/// Panics if `path` is not a valid `VfsPath`.
pub fn stub_request_context_at(path: &str) -> StubRequestContext {
    StubRequestContext {
        path: vfs(path),
        real_fs: StubFs,
        events: StubEvents,
        resolver: StubResolver,
        file_generations: FileGenerations::new(),
    }
}

/// Load a test fixture file from `src/{module}/fixtures/{name}`.
///
/// Panics if the file doesn't exist or can't be read — fixture absence
/// is always a test setup bug.
pub fn load_fixture(module: &str, name: &str) -> String {
    let path = format!("{}/src/{module}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read fixture {name}: {e}"))
}
