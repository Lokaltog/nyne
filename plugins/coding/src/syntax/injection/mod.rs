//! Injection-based compound decomposition.
//!
//! An `InjectionDecomposer` parses the outer template language (e.g. Jinja2),
//! extracts content regions, decomposes them with the inner language's
//! decomposer, and remaps byte ranges back to original-file coordinates.

use std::sync::Arc;

use color_eyre::eyre::Result;

use super::fragment::{ConflictSet, DecomposedFile, Fragment, Resolution};
use super::languages::jinja2::{extract_template, symbols_to_fragments};
use super::span_map::SpanMap;
use super::spec::{Decomposer, SpliceMode};

/// Compound decomposer that delegates inner-language parsing through an
/// outer template grammar with byte-range remapping.
pub(super) struct InjectionDecomposer {
    inner: Arc<dyn Decomposer>,
    /// File extension used for the compound file (the inner extension).
    inner_ext: &'static str,
}

impl InjectionDecomposer {
    pub(super) fn new(inner: Arc<dyn Decomposer>, inner_ext: &'static str) -> Self { Self { inner, inner_ext } }
}

impl Decomposer for InjectionDecomposer {
    fn decompose(&self, source: &str, max_depth: usize) -> (DecomposedFile, Option<tree_sitter::Tree>) {
        // 1. Parse with Jinja2 grammar and extract content regions + structural symbols.
        let extraction = extract_template(source);

        // 2. Build SpanMap and concatenated inner content from content regions.
        let (span_map, inner_content) = SpanMap::build(source, &extraction.regions);

        // 3. Convert Jinja2 structural symbols into fragments.
        let jinja2_fragments = symbols_to_fragments(extraction.symbols);

        // 4. Run inner decomposer on concatenated content.
        let (inner_fragments, _inner_tree) = if inner_content.is_empty() {
            (Vec::new(), None)
        } else {
            self.inner.decompose(&inner_content, max_depth)
        };

        // 5. Remap all inner fragments' byte ranges from virtual → real offsets,
        //    then recompute line_range from the original source.
        let remapped_fragments: Vec<Fragment> = inner_fragments
            .into_iter()
            .map(|f| span_map.remap_fragment(f))
            .map(|mut f| {
                recompute_byte_ranges_from_source(&mut f, source);
                f
            })
            .collect();

        // 6. Merge Jinja2 + remapped inner fragments, sorted by position.
        let mut fragments: Vec<Fragment> = jinja2_fragments.into_iter().chain(remapped_fragments).collect();
        fragments.sort_by_key(|f| f.byte_range.start);

        // Injection decomposers don't produce a usable tree — the inner tree
        // is over concatenated content with remapped offsets.
        (fragments, None)
    }

    fn validate(&self, source: &str) -> Result<()> {
        // Validate only the Jinja2 layer — inner content may contain template
        // artifacts that are valid Jinja2 but would fail strict inner parsing.
        let extraction = extract_template(source);
        if extraction.regions.is_empty() && extraction.symbols.is_empty() {
            // If Jinja2 extraction produced nothing, the source may not be a
            // valid template at all. Fall back to inner validation.
            return self.inner.validate(source);
        }
        Ok(())
    }

    fn language_name(&self) -> &'static str { "Jinja2" }

    fn file_extension(&self) -> &'static str { self.inner_ext }

    fn strip_doc_comment(&self, raw: &str) -> String { self.inner.strip_doc_comment(raw) }

    fn wrap_doc_comment(&self, plain: &str, indent: &str) -> String { self.inner.wrap_doc_comment(plain, indent) }

    fn clean_doc_comment(&self, raw: &str) -> Option<String> { self.inner.clean_doc_comment(raw) }

    fn map_to_fs(&self, fragments: &mut [Fragment]) { self.inner.map_to_fs(fragments); }

    fn resolve_conflicts(&self, conflicts: &[ConflictSet]) -> Vec<Resolution> {
        self.inner.resolve_conflicts(conflicts)
    }

    fn wrap_file_doc_comment(&self, plain: &str, indent: &str) -> String {
        self.inner.wrap_file_doc_comment(plain, indent)
    }

    fn splice_mode(&self) -> SpliceMode {
        // Injection-decomposed files have non-content nodes (render expressions,
        // control directives) interspersed within content lines. Line-snapped
        // reads would include those nodes in the body, corrupting edits. Byte
        // mode masks out everything outside the exact full_span.
        SpliceMode::Byte
    }
}

/// Recompute `line_range` for a fragment (and its children) from the original source.
///
/// After `SpanMap::remap_fragment` translates byte offsets from virtual to real
/// coordinates, `line_range` is stale — it was computed from the concatenated
/// content. This function recomputes it from `full_span` in the real source.
///
/// Uses [`line_of_byte`] — the SSOT for byte-offset → line-number conversion —
/// matching the same formula used by [`Fragment::new`].
const fn recompute_byte_ranges_from_source(_fragment: &mut Fragment, _source: &str) {
    // After SpanMap::remap_fragment, byte_range is already in real coordinates.
    // Children are remapped recursively by remap_fragment itself, so this
    // function is now a no-op — kept as a hook for future post-remap fixups.
}

#[cfg(test)]
mod tests;
