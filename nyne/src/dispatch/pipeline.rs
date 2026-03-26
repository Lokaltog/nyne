//! Middleware pipeline for read/write request processing.
//!
//! The pipeline executes a three-tier middleware chain for both reads and writes:
//! **node** (innermost) -> **provider** -> **global** (outermost). This ordering
//! means node-specific transformations run closest to the raw data, while global
//! cross-cutting concerns (e.g., metrics, validation) wrap everything.
//!
//! For writes, after the middleware chain and the final `Writable` dispatch,
//! post-write hooks run non-fatally -- the write is already committed, so hook
//! failures are logged but do not roll back the operation.

use crate::dispatch::WriteMode;
use crate::dispatch::context::PipelineContext;
use crate::node::WriteOutcome;
use crate::node::middleware::{PostWriteHook, ReadMiddleware, WriteMiddleware};
use crate::prelude::*;

/// Middleware pipeline that executes read/write chains.
///
/// Owns the global middlewares and post-write hooks. Node-level and
/// provider-level middlewares are discovered from the respective traits.
#[derive(Default)]
pub(super) struct Pipeline {
    global_read_middlewares: Vec<Box<dyn ReadMiddleware>>,
    global_write_middlewares: Vec<Box<dyn WriteMiddleware>>,
    post_write_hooks: Vec<Box<dyn PostWriteHook>>,
}

/// Pipeline construction and execution for read/write request chains.
impl Pipeline {
    /// Create an empty pipeline with no middlewares or hooks.
    pub(super) fn new() -> Self {
        Self {
            global_read_middlewares: Vec::new(),
            global_write_middlewares: Vec::new(),
            post_write_hooks: Vec::new(),
        }
    }

    /// Execute the full read pipeline for a node.
    ///
    /// Runs: `node.readable().read()` -> node middlewares -> provider middlewares -> global middlewares.
    pub(super) fn execute_read(
        &self,
        node: &VirtualNode,
        provider: &dyn Provider,
        ctx: &RequestContext<'_>,
    ) -> Result<Vec<u8>> {
        let mut data = node.require_readable()?.read(ctx)?;
        let mut pctx = PipelineContext::new(ctx);

        // Middleware pipeline: node (innermost) → provider → global (outermost).
        let provider_mws = provider.read_middlewares();
        for mw in middleware_chain(node.read_middlewares(), &provider_mws, &self.global_read_middlewares) {
            data = mw.process_read(data, &mut pctx)?;
        }

        Ok(data)
    }

    /// Execute the full write pipeline for a node.
    ///
    /// Runs: node middlewares -> provider middlewares -> global middlewares ->
    /// `node.writable().write()` (dispatched by mode) -> post-write hooks (non-fatal).
    pub(super) fn execute_write(
        &self,
        node: &VirtualNode,
        provider: &dyn Provider,
        data: &[u8],
        mode: WriteMode,
        ctx: &RequestContext<'_>,
    ) -> Result<WriteOutcome> {
        let writable = node.require_writable()?;
        let mut pctx = PipelineContext::new(ctx);
        let mut data = data.to_vec();

        // Middleware pipeline: node (innermost) → provider → global (outermost).
        let provider_mws = provider.write_middlewares();
        for mw in middleware_chain(node.write_middlewares(), &provider_mws, &self.global_write_middlewares) {
            data = mw.process_write(data, &mut pctx)?;
        }

        // Dispatch to the appropriate Writable method by mode.
        let outcome = match mode {
            WriteMode::Truncate => writable.truncate_write(ctx, &data)?,
            WriteMode::Normal => writable.write(ctx, &data)?,
        };

        // Post-write hooks (non-fatal — write already committed).
        for hook in &self.post_write_hooks {
            if let Err(e) = hook.after_write(&pctx) {
                tracing::warn!("post-write hook failed: {e}");
            }
        }

        Ok(outcome)
    }
}
/// Build the three-tier middleware chain: node (innermost) → provider → global (outermost).
///
/// Extracted to deduplicate the identical chain construction in `execute_read` and
/// `execute_write`.
fn middleware_chain<'a, T: ?Sized>(
    node_mws: &'a [Box<T>],
    provider_mws: &'a [Box<T>],
    global_mws: &'a [Box<T>],
) -> impl Iterator<Item = &'a T> {
    node_mws
        .iter()
        .chain(provider_mws.iter())
        .chain(global_mws.iter())
        .map(|b| &**b)
}
