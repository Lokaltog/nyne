//! Slice boundary decorators for virtual file reads and writes.
//!
//! Content slices (symbol bodies, signatures, docstrings) don't inherently end
//! with `\n`, but editors expect it. This decorator pair appends `\n` on read
//! and strips it on write, ensuring symmetric round-tripping at slice boundaries.

use color_eyre::eyre::Result;

use crate::router::{AffectedFiles, LazyReadable, NamedNode, Node, ReadContext, Readable, Writable, WriteContext};

/// Readable decorator: appends a trailing `\n` at the slice boundary.
///
/// Always appends unconditionally so the pair with [`SliceWritable`] is
/// symmetric: read adds one `\n`, write removes one `\n`. A conditional
/// append (only when missing) breaks round-tripping for content whose source
/// range already ends with `\n`.
pub struct SliceReadable<R>(pub R);

impl<R: Readable> Readable for SliceReadable<R> {
    fn read(&self, ctx: &ReadContext<'_>) -> Result<Vec<u8>> {
        let mut data = self.0.read(ctx)?;
        data.push(b'\n');
        Ok(data)
    }
}

/// Writable decorator: strips exactly one trailing `\n` before writing.
///
/// Undoes the `\n` that [`SliceReadable`] added on read, so the write handler
/// receives content matching the original byte range.
pub struct SliceWritable<W>(pub W);

impl<W: Writable> Writable for SliceWritable<W> {
    fn write(&self, ctx: &WriteContext<'_>, data: &[u8]) -> Result<AffectedFiles> {
        self.0.write(ctx, data.strip_suffix(b"\n").unwrap_or(data))
    }
}

/// Build an editable file node with slice boundary decoration.
pub fn slice_node(
    name: impl Into<String>,
    readable: impl Readable + 'static,
    writable: impl Writable + 'static,
) -> NamedNode {
    Node::file()
        .with_readable(SliceReadable(readable))
        .with_writable(SliceWritable(writable))
        .named(name)
}

/// Build an editable file node with a closure-backed reader and slice boundary decoration.
///
/// Convenience wrapper: the read closure is wrapped in [`LazyReadable`](crate::router::LazyReadable)
/// internally.
pub fn lazy_slice_node(
    name: impl Into<String>,
    read_fn: impl for<'a> Fn(&ReadContext<'a>) -> Result<Vec<u8>> + Send + Sync + 'static,
    writable: impl Writable + 'static,
) -> NamedNode {
    slice_node(name, LazyReadable::new(read_fn), writable)
}
