use std::str::from_utf8;

use color_eyre::eyre::Result;
use nyne::dispatch::context::RequestContext;
use nyne::node::capabilities::Writable;
use nyne::node::kind::WriteOutcome;

use super::meta::MetaSplice;
use crate::syntax::spec::SpliceMode;

/// Writable body that splices content back into the source file.
///
/// On write: reads the current source, replaces the full span with the new
/// content, validates the result with tree-sitter, and writes back to disk.
/// Rejects with `EINVAL` if the spliced result has parse errors.
pub(in crate::providers::syntax) struct BodySplice {
    pub meta: MetaSplice,
}

/// [`Writable`] implementation for [`BodySplice`].
impl Writable for BodySplice {
    /// Validate syntax and splice the new body into the source file.
    fn write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> {
        let content = from_utf8(data)?;
        let shared = self.meta.resolver.decompose()?;
        let content = match shared.decomposer.splice_mode() {
            SpliceMode::Line => content,
            // Byte mode: the read path pads with spaces to fill lines, so
            // strip leading/trailing whitespace to recover the actual
            // expression before splicing back at the exact byte range.
            SpliceMode::Byte => content.trim(),
        };
        self.meta.splice_write(ctx, content)
    }

    /// Delegate truncate-write to the standard write path.
    fn truncate_write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> { self.write(ctx, data) }
}
