//! `#[cfg(test)]` utilities for router tests.

use std::path::Path;

use color_eyre::eyre::Result;

use crate::router::fs::mem::MemFs;
use crate::router::node::{AffectedFiles, ReadContext, Readable, Writable, WriteContext};
use crate::router::{Next, Node, Provider, ProviderId, ProviderMeta, Request};

/// A readable that returns static content.
#[derive(Clone)]
pub struct StubReadable(Vec<u8>);

impl StubReadable {
    pub fn new(content: &str) -> Self { Self(content.as_bytes().to_vec()) }
}

impl Readable for StubReadable {
    fn read(&self, _ctx: &ReadContext<'_>) -> Result<Vec<u8>> { Ok(self.0.clone()) }
}

/// A writable that discards all content.
pub struct StubWritable;

impl Writable for StubWritable {
    fn write(&self, _ctx: &WriteContext<'_>, _data: &[u8]) -> Result<AffectedFiles> { Ok(vec![]) }
}

/// A provider that stops the chain and emits `stopped.txt`.
pub struct StoppingProvider {
    id: ProviderId,
}

impl StoppingProvider {
    pub fn new() -> Self {
        Self {
            id: ProviderId::new("stopper"),
        }
    }
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

/// Create a `ReadContext` backed by an empty `MemFs` for testing.
pub fn test_read_ctx() -> ReadContext<'static> {
    ReadContext {
        path: Path::new(""),
        // Leak the MemFs so we get a &'static reference for tests.
        fs: Box::leak(Box::new(MemFs::new())),
    }
}
