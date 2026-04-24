//! Shared test fixtures and stubs.
//!
//! Import with `use crate::test_support::*;` in any `tests.rs` module.
//! This is the single source of truth for common test helpers — never
//! duplicate these in individual test modules.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use color_eyre::eyre::{Result, bail};

use crate::config::NyneConfig;
use crate::dispatch::activation::ActivationContext;
use crate::process::{ProcessNameCache, Spawner};
use crate::router::{
    AffectedFiles, DirEntry, Filesystem, MemFs, Metadata, Next, Node, Provider, ProviderId, ProviderMeta, ReadContext,
    Readable, Request, Writable, WriteContext,
};

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
        Arc::new(ProcessNameCache::default()),
    )
}
/// A readable that returns static content.
///
/// Supports optional backing path (`.with_backing(path)`) to simulate a
/// real on-disk file, and optional call counting (`.with_counter()`) to
/// let tests observe read invocations across ownership boundaries.
#[derive(Clone, Default)]
pub struct StubReadable {
    content: Vec<u8>,
    backing_path: Option<PathBuf>,
    call_count: Option<Arc<AtomicU32>>,
}

impl StubReadable {
    /// Empty readable — returns no content, no backing path.
    pub fn empty() -> Self { Self::default() }

    /// Readable returning the given text content.
    pub fn new(content: &str) -> Self {
        Self {
            content: content.as_bytes().to_vec(),
            ..Self::default()
        }
    }

    /// Readable returning the given byte content.
    pub fn from_bytes(content: &[u8]) -> Self {
        Self {
            content: content.to_vec(),
            ..Self::default()
        }
    }

    /// Attach a backing path so the readable reports as file-backed.
    pub fn with_backing(mut self, path: impl Into<PathBuf>) -> Self {
        self.backing_path = Some(path.into());
        self
    }

    /// Enable call counting. Returns `(self, counter)` — the returned
    /// `Arc<AtomicU32>` observes read calls even after `self` is moved
    /// into a node.
    pub fn with_counter(mut self) -> (Self, Arc<AtomicU32>) {
        let counter = Arc::new(AtomicU32::new(0));
        self.call_count = Some(Arc::clone(&counter));
        (self, counter)
    }
}

impl Readable for StubReadable {
    fn read(&self, _ctx: &ReadContext<'_>) -> Result<Vec<u8>> {
        if let Some(c) = &self.call_count {
            c.fetch_add(1, Ordering::Relaxed);
        }
        Ok(self.content.clone())
    }

    fn backing_path(&self) -> Option<&Path> { self.backing_path.as_deref() }
}

/// A writable that discards all content.
pub struct StubWritable;

impl Writable for StubWritable {
    fn write(&self, _ctx: &WriteContext<'_>, _data: &[u8]) -> Result<AffectedFiles> { Ok(vec![]) }
}
/// A writable that records every byte payload written to it.
///
/// Use when a test needs to assert on the data a splicing or middleware
/// writable forwarded downstream. [`last_write`] returns the most recent
/// buffer as a UTF-8 string (panicking on non-UTF-8 for test ergonomics).
#[derive(Default)]
pub struct RecordingWritable(std::sync::Mutex<Vec<u8>>);

impl RecordingWritable {
    /// Construct an empty recorder.
    pub fn new() -> Self { Self::default() }

    /// Return the last recorded write as a UTF-8 string.
    pub fn last_write(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).expect("recorded write is valid UTF-8")
    }

    /// Return the last recorded write as raw bytes.
    pub fn last_write_bytes(&self) -> Vec<u8> { self.0.lock().unwrap().clone() }
}

impl Writable for RecordingWritable {
    fn write(&self, _ctx: &WriteContext<'_>, data: &[u8]) -> Result<AffectedFiles> {
        *self.0.lock().unwrap() = data.to_vec();
        Ok(vec![])
    }
}


/// A provider that stops the chain and emits `stopped.txt`.
pub struct StoppingProvider {
    id: ProviderId,
}

impl StoppingProvider {
    pub const fn new() -> Self {
        Self {
            id: ProviderId::new("stopper"),
        }
    }
}

impl Default for StoppingProvider {
    fn default() -> Self { Self::new() }
}

impl ProviderMeta for StoppingProvider {
    fn id(&self) -> ProviderId { self.id }

    fn terminal(&self) -> bool { true }
}

impl Provider for StoppingProvider {
    fn accept(&self, req: &mut Request, _next: &Next) -> Result<()> {
        req.nodes.add(Node::file().named("stopped.txt"));
        Ok(())
    }
}

/// Create a [`ReadContext`] backed by an empty [`MemFs`] for testing.
pub fn test_read_ctx() -> ReadContext<'static> {
    ReadContext {
        path: Path::new(""),
        // Leak the MemFs so we get a &'static reference for tests.
        fs: Box::leak(Box::new(MemFs::new())),
    }
}
