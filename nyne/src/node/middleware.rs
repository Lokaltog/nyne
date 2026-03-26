//! Read/write middleware pipeline for node content transformations.
//!
//! Middlewares are attached to nodes at construction time and run by the
//! dispatch pipeline in order. They transform content bytes without changing
//! the node's core [`Readable`](super::Readable)/[`Writable`](super::Writable)
//! logic — useful for cross-cutting concerns like line-number injection,
//! encoding normalization, or content post-processing.
//!
//! The pipeline runs middlewares in attachment order: write middlewares
//! transform data *before* it reaches the writable, read middlewares
//! transform data *after* it leaves the readable.

use color_eyre::eyre::Result;

use crate::dispatch::context::PipelineContext;

/// Middleware applied to data before it is written to a node.
///
/// Receives the raw bytes from the FUSE write and returns (possibly
/// transformed) bytes that will be passed to the node's
/// [`Writable::write`](super::Writable::write). Returning an error
/// aborts the write and surfaces the error to the caller.
pub trait WriteMiddleware: Send + Sync {
    /// Transform write data before it reaches the node's writable.
    fn process_write(&self, data: Vec<u8>, ctx: &mut PipelineContext<'_>) -> Result<Vec<u8>>;
}

/// Middleware applied to data after it is read from a node.
///
/// Receives the bytes produced by the node's
/// [`Readable::read`](super::Readable::read) and returns (possibly
/// transformed) bytes that will be served to the FUSE read response.
pub trait ReadMiddleware: Send + Sync {
    /// Transform read data before it is returned to the caller.
    fn process_read(&self, data: Vec<u8>, ctx: &mut PipelineContext<'_>) -> Result<Vec<u8>>;
}

/// Hook invoked after a write operation completes successfully.
///
/// Unlike [`WriteMiddleware`], this does not transform data — it runs
/// side effects (e.g., invalidating dependent nodes, triggering re-parse)
/// after the write has been committed.
pub trait PostWriteHook: Send + Sync {
    /// Run post-write side effects.
    fn after_write(&self, ctx: &PipelineContext<'_>) -> Result<()>;
}
