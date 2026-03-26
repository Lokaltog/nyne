//! Line-range slicing plugin for `:M-N` suffixes.
//!
//! Enables reading, writing, and deleting subsets of a file's lines via
//! path suffixes like `file.rs@/lines:10-20`. The [`LineSlice`] plugin
//! attaches to a base node and derives virtual slice nodes on demand when
//! the dispatch layer encounters a lookup miss matching `base:M-N` syntax.
//!
//! Sliced writes work by read-modify-write: the full base content is read,
//! the targeted lines are spliced, and the complete result is written back
//! through the base node's [`Writable`]. This ensures that any validation
//! (e.g., tree-sitter syntax checking) runs on the full file content.

use std::sync::Arc;

use color_eyre::eyre::{Result, bail};

use super::plugin::NodePlugin;
use super::{Readable, Unlinkable, VirtualNode, Writable, WriteOutcome};
use crate::dispatch::context::RequestContext;
use crate::types::slice::{SliceSpec, parse_slice_suffix};

/// Plugin that enables `:M-N` line slicing on any readable node.
///
/// Attach to a node via `.sliceable()`. When the dispatch layer
/// encounters a lookup miss for `"BLAME.md:5-20"` and the base
/// node `"BLAME.md"` has this plugin, it derives a sliced variant.
///
/// If the base node is writable, the derived node is also writable —
/// writes splice new content at the specified line range and delegate
/// the full result through the base node's writable.
pub struct LineSlice;

/// Plugin derivation for line-sliced nodes.
impl NodePlugin for LineSlice {
    /// Derives a sliced node if the name matches `base:M-N` syntax.
    fn derive(&self, base: &Arc<VirtualNode>, name: &str, _ctx: &RequestContext<'_>) -> Result<Option<VirtualNode>> {
        let Some((base_name, spec)) = parse_slice_suffix(name) else {
            return Ok(None);
        };

        if base_name != base.name() {
            return Ok(None);
        }

        let mut node = VirtualNode::file(name, SlicedReadable {
            base: Arc::clone(base),
            spec,
        })
        .hidden();

        if base.writable().is_some() {
            node = node
                .with_writable(SlicedWritable {
                    base: Arc::clone(base),
                    spec,
                })
                .with_unlinkable(SlicedUnlinkable {
                    base: Arc::clone(base),
                    spec,
                });
        }

        Ok(Some(node))
    }
}

/// Lazily reads the base node's content and extracts a line range [DD-5].
struct SlicedReadable {
    base: Arc<VirtualNode>,
    spec: SliceSpec,
}

/// Readable implementation for line-sliced content.
impl Readable for SlicedReadable {
    /// Reads the base content and extracts the specified line range.
    fn read(&self, ctx: &RequestContext<'_>) -> Result<Vec<u8>> {
        let content = self.base.require_readable()?.read(ctx)?;
        let range = self.spec.index_range(split_lines(&content).count());
        let mut result = Vec::new();
        let mut first = true;
        for line in split_lines(&content).skip(range.start).take(range.len()) {
            if !first {
                result.push(b'\n');
            }
            result.extend_from_slice(line);
            first = false;
        }
        Ok(result)
    }
}

/// Writes to a sliced line range by splicing into the base node's content.
///
/// Reads current content from the base readable, replaces the targeted
/// lines with the new data, and writes the full result through the base
/// writable. Validation (e.g. syntax checking) is handled by the base
/// writable — this type is purely a splice adapter.
struct SlicedWritable {
    base: Arc<VirtualNode>,
    spec: SliceSpec,
}

/// Writable implementation for line-sliced content.
impl Writable for SlicedWritable {
    /// Splices new data into the base content at the specified line range.
    fn write(&self, ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> {
        let current = self.base.require_readable()?.read(ctx)?;
        self.base
            .require_writable()?
            .write(ctx, &splice_lines(&current, &self.spec, data)?)
    }
}
/// Removes a sliced line range by splicing empty data into the base node.
///
/// Semantically identical to writing empty content through [`SlicedWritable`],
/// but triggered by `rm` (unlink) instead of truncate-then-close.
struct SlicedUnlinkable {
    base: Arc<VirtualNode>,
    spec: SliceSpec,
}

/// Unlinkable implementation that deletes a line range.
impl Unlinkable for SlicedUnlinkable {
    /// Removes the specified line range from the base content.
    fn unlink(&self, ctx: &RequestContext<'_>) -> Result<()> {
        let current = self.base.require_readable()?.read(ctx)?;
        self.base
            .require_writable()?
            .write(ctx, &splice_lines(&current, &self.spec, b"")?)?;
        Ok(())
    }
}

/// Splice `new_data` into `current` at the line range specified by `spec`.
///
/// Uses byte-level `\n` splitting (not `String::lines()`) for consistency
/// with the read path in [`SlicedReadable::read`]. Trailing `\n` is treated
/// as a line terminator, not a separator.
///
/// Returns an error if `new_data` is not valid UTF-8.
fn splice_lines(current: &[u8], spec: &SliceSpec, new_data: &[u8]) -> Result<Vec<u8>> {
    if str::from_utf8(new_data).is_err() {
        bail!("line splice data is not valid UTF-8");
    }

    let has_trailing_newline = current.last() == Some(&b'\n');
    let range = spec.index_range(split_lines(current).count());

    let before = split_lines(current).take(range.start);
    let after = split_lines(current).skip(range.end);
    let replacement = split_lines(new_data);

    let mut out = Vec::with_capacity(current.len());
    let mut first = true;
    for line in before.chain(replacement).chain(after) {
        if !first {
            out.push(b'\n');
        }
        out.extend_from_slice(line);
        first = false;
    }
    if has_trailing_newline {
        out.push(b'\n');
    }
    Ok(out)
}

/// Return an iterator over lines in `data`, treating `\n` as a terminator.
///
/// Unlike `slice::split`, strips the trailing empty element produced by
/// a terminating `\n`, and yields nothing for empty input.
fn split_lines(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    let trimmed = data.strip_suffix(b"\n").unwrap_or(data);
    // `split` on empty input yields one empty slice; skip it.
    trimmed
        .split(|&b| b == b'\n')
        .take(if trimmed.is_empty() { 0 } else { usize::MAX })
}

/// Unit tests.
#[cfg(test)]
mod tests;
