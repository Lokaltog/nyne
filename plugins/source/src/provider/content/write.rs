//! Symbol body write and splice operations.
//!
//! Validates new content with tree-sitter before splicing it back into the
//! source file, rejecting writes that would introduce parse errors.

use std::str::from_utf8;

use color_eyre::eyre::Result;
use nyne::router::{AffectedFiles, Writable, WriteContext};

use super::meta::MetaSplice;
use crate::syntax::spec::SpliceMode;

/// Writable body that splices content back into the source file.
///
/// On write: reads the current source, replaces the full span with the new
/// content, validates the result with tree-sitter, and writes back to disk.
/// Rejects with `EINVAL` if the spliced result has parse errors.
pub(in crate::provider) struct BodySplice {
    pub meta: MetaSplice,
}

impl Writable for BodySplice {
    /// Validate syntax and splice the new body into the source file.
    fn write(&self, ctx: &WriteContext<'_>, data: &[u8]) -> Result<AffectedFiles> {
        let content = from_utf8(data)?;
        let shared = self.meta.resolver.decompose()?;
        let content = match shared.decomposer.splice_mode() {
            SpliceMode::Line => content,
            // Byte mode: the read path pads with spaces to fill lines, so
            // strip leading/trailing whitespace to recover the actual
            // expression before splicing back at the exact byte range.
            SpliceMode::Byte => content.trim(),
        };
        self.meta.splice_write(ctx.fs, content)
    }
}
