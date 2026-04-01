mod readables;
pub mod state;

use std::sync::Arc;

use color_eyre::eyre::Result;
use nyne::router::{Next, Op, Provider, Request};
use readables::{DefaultSplicingWritable, LineSlice, SplicingWritable, StaticContent};
pub use state::*;

/// Path-rewriting middleware that makes `:{spec}` work on any readable node.
///
/// Strips the `:{spec}` suffix, sets [`SliceSpec`] as request state, lets
/// downstream resolve the base file, then applies slicing:
///
/// - If `req.state::<Sliceable>()` is present (opt-in by a content producer's
///   handler), calls its `slice` callback for custom range semantics.
/// - Otherwise, wraps the resolved [`Readable`] with default line-range slicing.
pub struct SliceProvider;

nyne::define_provider!(SliceProvider, "slice", priority: -100);

impl Provider for SliceProvider {
    fn accept(&self, req: &mut Request, next: &Next) -> Result<()> {
        let Op::Lookup { ref name } = *req.op() else {
            return next.run(req);
        };

        let Some((base_name, spec)) = parse_slice_suffix(name) else {
            return next.run(req);
        };

        let range = parse_range(spec)?;
        let base_name = base_name.to_owned();
        let full_name = name.to_owned();

        // Two-phase lookup: the full name must be tried first because colons
        // can appear in real entry names (e.g. LSP reference symlinks like
        // `main.rs:13`). Reversing the order was tried previously and broke
        // those symlinks — when the base name (`main.rs`) also existed as an
        // entry, it was incorrectly sliced instead of resolving the literal
        // symlink.
        //
        // The cache middleware must NOT restore request state for negative
        // hits (see cache provider), otherwise the speculative lookup here
        // leaks companion state into the base-name lookup below, poisoning
        // the cache and causing permanent ENOENT.
        next.run(req)?;

        if !req.nodes.is_empty() {
            return Ok(());
        }

        // Full name didn't resolve — treat as base_name + slice spec.
        req.set_state(range);
        req.set_op(Op::Lookup {
            name: base_name.clone(),
        });
        next.run(req)?;

        if req.nodes.is_empty() {
            return Ok(());
        }

        apply_slicing(req, &base_name, &full_name, range)
    }
}

/// Post-processing: decorate the resolved node with slicing/splicing capabilities.
fn apply_slicing(req: &mut Request, base_name: &str, full_name: &str, range: SliceSpec) -> Result<()> {
    // Extract Sliceable results before taking a mutable node reference (borrow split).
    let custom_content = req
        .state::<Sliceable>()
        .map(|s| (s.slice)(range.start, range.end.unwrap_or(usize::MAX)))
        .transpose()?;
    let custom_splice = req.state::<Sliceable>().and_then(|s| s.splice.clone());

    let Some(node) = req.nodes.find_mut(base_name) else {
        return Ok(());
    };

    // Read side: custom slice content or default line-range slicing.
    // Keep the original readable around for the default write-splice path.
    let original_readable = if custom_content.is_some() {
        None
    } else {
        node.take_readable()
    };
    if let Some(content) = custom_content {
        node.set_readable(StaticContent(content));
    } else if let Some(ref inner) = original_readable {
        node.set_readable(LineSlice {
            inner: Arc::clone(inner),
            start: range.start,
            end: range.end,
        });
    }

    // Write side: custom splice callback, or default line-range splice
    // that reads current content, replaces the target lines, and writes back.
    if let Some(splice) = custom_splice {
        node.set_writable(SplicingWritable {
            splice,
            start: range.start,
            end: range.end,
        });
    } else if let Some(inner_writable) = node.take_writable()
        && let Some(inner_readable) = original_readable
    {
        node.set_writable(DefaultSplicingWritable {
            readable: inner_readable,
            writable: inner_writable,
            start: range.start,
            end: range.end,
        });
    }

    node.set_name(full_name.to_owned());
    Ok(())
}

#[cfg(test)]
mod tests;
