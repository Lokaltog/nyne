use std::sync::Arc;

use color_eyre::eyre::Result;

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
        let lines: Vec<&[u8]> = content.split(|&b| b == b'\n').collect();
        let sliced = self.spec.apply(&lines);
        let mut result = Vec::new();
        for (i, line) in sliced.iter().enumerate() {
            result.extend_from_slice(line);
            if i < sliced.len() - 1 {
                result.push(b'\n');
            }
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
            .write(ctx, &splice_lines(&current, &self.spec, data))
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
            .write(ctx, &splice_lines(&current, &self.spec, b""))?;
        Ok(())
    }
}

/// Splice `new_data` into `current` at the line range specified by `spec`.
///
/// Returns the full content with the targeted lines replaced.
fn splice_lines(current: &[u8], spec: &SliceSpec, new_data: &[u8]) -> Vec<u8> {
    let existing = String::from_utf8_lossy(current);
    let lines: Vec<&str> = existing.lines().collect();
    let range = spec.index_range(lines.len());

    let replacement = String::from_utf8_lossy(new_data);
    let new_lines: Vec<&str> = replacement.lines().collect();

    let mut result: Vec<&str> = Vec::with_capacity(lines.len() - range.len() + new_lines.len());
    result.extend_from_slice(lines.get(..range.start).unwrap_or(&[]));
    result.extend_from_slice(&new_lines);
    result.extend_from_slice(lines.get(range.end..).unwrap_or(&[]));

    let mut out = result.join("\n");
    // Preserve trailing newline if the original had one.
    if current.last() == Some(&b'\n') {
        out.push('\n');
    }
    out.into_bytes()
}

#[cfg(test)]
mod tests;
