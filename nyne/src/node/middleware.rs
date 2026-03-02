use color_eyre::eyre::Result;

use crate::dispatch::context::PipelineContext;

/// Middleware applied to data before it is written to a node.
pub trait WriteMiddleware: Send + Sync {
    fn process_write(&self, data: Vec<u8>, ctx: &mut PipelineContext<'_>) -> Result<Vec<u8>>;
}

/// Middleware applied to data after it is read from a node.
pub trait ReadMiddleware: Send + Sync {
    fn process_read(&self, data: Vec<u8>, ctx: &mut PipelineContext<'_>) -> Result<Vec<u8>>;
}

/// Hook invoked after a write operation completes.
pub trait PostWriteHook: Send + Sync {
    fn after_write(&self, ctx: &PipelineContext<'_>) -> Result<()>;
}
