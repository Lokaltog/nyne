use color_eyre::eyre::Result;
use nyne::dispatch::context::PipelineContext;
use nyne::node::VirtualNode;
use nyne::node::middleware::{ReadMiddleware, WriteMiddleware};

/// Read middleware: appends a synthetic trailing `\n`.
///
/// Syntax fragment files (body, signature, docstring, decorators, imports)
/// are sliced from the source by byte range. Editors expect files to end
/// with `\n` — without it, tools like neovim add one on write, creating a
/// mismatch that triggers infinite re-read/re-write cycles.
///
/// Always appends unconditionally so the pair with [`StripTrailingNewline`]
/// is symmetric: read adds one `\n`, write removes one `\n`. A conditional
/// append (only when missing) breaks round-tripping for fragments whose
/// source range already ends with `\n` — the read would be a no-op but the
/// write would still strip, losing the original newline.
pub(super) struct AppendTrailingNewline;

/// [`ReadMiddleware`] implementation for [`AppendTrailingNewline`].
impl ReadMiddleware for AppendTrailingNewline {
    /// Append a trailing newline byte to the read data.
    fn process_read(&self, mut data: Vec<u8>, _ctx: &mut PipelineContext<'_>) -> Result<Vec<u8>> {
        data.push(b'\n');
        Ok(data)
    }
}

/// Write middleware: strips exactly one trailing `\n` before splicing back.
///
/// Undoes the `\n` that [`AppendTrailingNewline`] added on read, so the
/// splice handler receives content matching the original byte range.
pub(super) struct StripTrailingNewline;

/// [`WriteMiddleware`] implementation for [`StripTrailingNewline`].
impl WriteMiddleware for StripTrailingNewline {
    /// Strip exactly one trailing newline byte before passing data downstream.
    fn process_write(&self, mut data: Vec<u8>, _ctx: &mut PipelineContext<'_>) -> Result<Vec<u8>> {
        if data.ends_with(b"\n") {
            data.pop();
        }
        Ok(data)
    }
}

/// Attach the trailing-newline middleware pair to a [`VirtualNode`].
///
/// Single call site for the convention — keeps the read/write pairing
/// in one place so they can't drift apart.
pub(super) fn with_newline_middlewares(node: VirtualNode) -> VirtualNode {
    node.with_read_middlewares(vec![Box::new(AppendTrailingNewline)])
        .with_write_middlewares(vec![Box::new(StripTrailingNewline)])
}

#[cfg(test)]
mod tests;
