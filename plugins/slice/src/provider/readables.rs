use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::router::{AffectedFiles, ReadContext, Readable, Writable, WriteContext};

use super::state::{SliceSpec, SpliceFn};

/// Readable that returns static content (used after custom Sliceable produces bytes).
pub(super) struct StaticContent(pub(super) Vec<u8>);
impl Readable for StaticContent {
    fn read(&self, _ctx: &ReadContext<'_>) -> Result<Vec<u8>> { Ok(self.0.clone()) }
}
/// Readable decorator that extracts a line range from the inner Readable.
pub(super) struct LineSlice {
    pub(super) inner: Arc<dyn Readable>,
    pub(super) start: usize,
    pub(super) end: Option<usize>,
}
impl Readable for LineSlice {
    fn read(&self, ctx: &ReadContext<'_>) -> Result<Vec<u8>> {
        let content = self.inner.read(ctx)?;
        let text = String::from_utf8_lossy(&content);
        let lines: Vec<&str> = text.lines().collect();

        let spec = SliceSpec {
            start: self.start,
            end: self.end,
        };
        let range = spec.line_range(lines.len());

        let selected: Vec<&str> = match lines.get(range) {
            Some(slice) => slice.to_vec(),
            None => return Ok(Vec::new()),
        };
        let mut result = selected.join("\n");
        // Preserve trailing newline if original had one
        if content.ends_with(b"\n") || !selected.is_empty() {
            result.push('\n');
        }
        Ok(result.into_bytes())
    }
}
pub(super) struct SplicingWritable {
    pub(super) splice: SpliceFn,
    pub(super) start: usize,
    pub(super) end: Option<usize>,
}
impl Writable for SplicingWritable {
    fn write(&self, _ctx: &WriteContext<'_>, data: &[u8]) -> Result<AffectedFiles> {
        (self.splice)(self.start, self.end.unwrap_or(self.start), data)
    }
}
/// Default line-range splice writable for nodes that have a writable but no
/// custom [`Sliceable`] splice. Reads current content from the original
/// readable, replaces the target line range with new data, and writes the full
/// result through the original writable.
pub(super) struct DefaultSplicingWritable {
    pub(super) readable: Arc<dyn Readable>,
    pub(super) writable: Arc<dyn Writable>,
    pub(super) start: usize,
    pub(super) end: Option<usize>,
}
impl Writable for DefaultSplicingWritable {
    fn write(&self, ctx: &WriteContext<'_>, data: &[u8]) -> Result<AffectedFiles> {
        let read_ctx = ReadContext {
            path: ctx.path,
            fs: ctx.fs,
        };
        let current = self.readable.read(&read_ctx)?;
        let current_text = String::from_utf8_lossy(&current);
        let mut lines: Vec<&str> = current_text.lines().collect();

        let new_text = String::from_utf8_lossy(data);
        let new_lines: Vec<&str> = new_text.lines().collect();

        let spec = SliceSpec {
            start: self.start,
            end: self.end,
        };
        lines.splice(spec.line_range(lines.len()), new_lines);

        let mut result = lines.join("\n");
        if current.ends_with(b"\n") || !result.is_empty() {
            result.push('\n');
        }
        self.writable.write(ctx, result.as_bytes())
    }
}
