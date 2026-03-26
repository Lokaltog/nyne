//! Injection-based compound decomposition.
//!
//! An `InjectionDecomposer` parses the outer template language (e.g. Jinja2),
//! extracts content regions, decomposes them with the inner language's
//! decomposer, and remaps byte ranges back to original-file coordinates.
//!
//! The pipeline for a compound file like `schema.sql.j2`:
//! 1. Parse with the outer grammar (Jinja2) to identify content vs directives.
//! 2. Concatenate content regions into a virtual byte space.
//! 3. Run the inner decomposer (SQL) on the concatenated content.
//! 4. Remap all resulting fragments' byte offsets back to real-file coordinates
//!    via [`SpanMap`](super::span_map::SpanMap).
//! 5. Merge Jinja2 structural symbols and remapped inner fragments.

use std::sync::Arc;

use color_eyre::eyre::Result;

use super::fragment::{ConflictSet, DecomposedFile, Fragment, Resolution};
use super::languages::jinja2::{extract_template, symbols_to_fragments};
use super::span_map::SpanMap;
use super::spec::{Decomposer, SpliceMode};

/// Compound decomposer that delegates inner-language parsing through an
/// outer template grammar with byte-range remapping.
///
/// For a file like `schema.sql.j2`, the outer grammar (Jinja2) identifies
/// content regions vs template directives. Content regions are concatenated
/// and handed to the inner decomposer (SQL). A [`SpanMap`] then remaps the
/// resulting fragments' byte ranges from the virtual concatenated space back
/// to their real positions in the original file.
pub(super) struct InjectionDecomposer {
    inner: Arc<dyn Decomposer>,
    /// File extension used for the compound file (the inner extension).
    inner_ext: &'static str,
}

/// Constructor for `InjectionDecomposer`.
impl InjectionDecomposer {
    /// Creates a new injection decomposer wrapping the given inner decomposer.
    pub(super) fn new(inner: Arc<dyn Decomposer>, inner_ext: &'static str) -> Self { Self { inner, inner_ext } }
}

/// [`Decomposer`] implementation for injection-based compound files.
impl Decomposer for InjectionDecomposer {
    /// Decomposes source by extracting Jinja2 structure then delegating inner content.
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

        // 5. Remap all inner fragments' byte ranges from virtual → real offsets.
        let remapped_fragments: Vec<Fragment> = inner_fragments
            .into_iter()
            .map(|f| span_map.remap_fragment(f))
            .collect();

        // 6. Merge Jinja2 + remapped inner fragments, sorted by position.
        let mut fragments: Vec<Fragment> = jinja2_fragments.into_iter().chain(remapped_fragments).collect();
        fragments.sort_by_key(|f| f.byte_range.start);

        // Injection decomposers don't produce a usable tree — the inner tree
        // is over concatenated content with remapped offsets.
        (fragments, None)
    }

    /// Validates the Jinja2 template layer, falling back to inner validation.
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

    /// Returns the language name for the injection layer.
    fn language_name(&self) -> &'static str { "Jinja2" }

    /// Returns the file extension of the inner language.
    fn file_extension(&self) -> &'static str { self.inner_ext }

    /// Delegates doc comment stripping to the inner decomposer.
    fn strip_doc_comment(&self, raw: &str) -> String { self.inner.strip_doc_comment(raw) }

    /// Delegates doc comment wrapping to the inner decomposer.
    fn wrap_doc_comment(&self, plain: &str, indent: &str) -> String { self.inner.wrap_doc_comment(plain, indent) }

    /// Delegates doc comment cleaning to the inner decomposer.
    fn clean_doc_comment(&self, raw: &str) -> Option<String> { self.inner.clean_doc_comment(raw) }

    /// Delegates filesystem mapping to the inner decomposer.
    fn map_to_fs(&self, fragments: &mut [Fragment]) { self.inner.map_to_fs(fragments); }

    /// Delegates conflict resolution to the inner decomposer.
    fn resolve_conflicts(&self, conflicts: &[ConflictSet]) -> Vec<Resolution> {
        self.inner.resolve_conflicts(conflicts)
    }

    /// Delegates file-level doc comment wrapping to the inner decomposer.
    fn wrap_file_doc_comment(&self, plain: &str, indent: &str) -> String {
        self.inner.wrap_file_doc_comment(plain, indent)
    }

    /// Returns byte-based splice mode to avoid corrupting interleaved template nodes.
    fn splice_mode(&self) -> SpliceMode {
        // Injection-decomposed files have non-content nodes (render expressions,
        // control directives) interspersed within content lines. Line-snapped
        // reads would include those nodes in the body, corrupting edits. Byte
        // mode masks out everything outside the exact full_span.
        SpliceMode::Byte
    }
}

/// Tests for injection-based compound decomposition.
#[cfg(test)]
mod tests;
