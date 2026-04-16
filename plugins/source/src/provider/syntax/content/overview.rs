//! Overview content types for symbol table rendering.
//!
//! All rendering is driven by [`FragmentView`] objects exposed as minijinja
//! [`Object`](minijinja::value::Object) instances — no intermediate row
//! types or Rust-side flattening.

use std::path::Path;

use color_eyre::eyre::Result;
use minijinja::value::Value;
use nyne::templates::{TemplateEngine, TemplateView};

use super::FragmentResolver;
use super::meta::FragmentPath;
use crate::syntax;
use crate::syntax::decomposed::DecomposedSource;
use crate::syntax::fragment::{FragmentKind, find_fragment_of_kind};
use crate::syntax::view::fragment_list;

/// View that renders the OVERVIEW.md template for a file's symbol table.
///
/// Resolves lazily via [`FragmentResolver`] — never stale after writes.
pub(in crate::provider::syntax) struct OverviewContent {
    pub resolver: FragmentResolver,
    pub filename: String,
}

impl TemplateView for OverviewContent {
    /// Render the companion overview with the full symbol table.
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let shared = self.resolver.decompose()?;
        let view = minijinja::context! {
            filename => &self.filename,
            file_doc => file_doc_text(&shared),
            fragments => fragment_list(&shared.decomposed, &shared),
        };
        Ok(engine.render_bytes(template, &view))
    }
}

/// View for a per-symbol OVERVIEW.md (lists child symbols).
///
/// Resolves lazily via [`FragmentResolver`] — never stale after writes.
pub(in crate::provider::syntax) struct SymbolOverviewContent {
    pub resolver: FragmentResolver,
    pub fragment_path: FragmentPath,
}

impl TemplateView for SymbolOverviewContent {
    /// Render the per-symbol overview listing child symbols.
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let shared = self.resolver.decompose()?;
        let frag = syntax::require_fragment(&shared.decomposed, &self.fragment_path)?;
        let view = minijinja::context! {
            filename => &frag.name,
            file_doc => Value::UNDEFINED,
            fragments => fragment_list(&frag.children, &shared),
        };
        Ok(engine.render_bytes(template, &view))
    }
}

/// View for the file-level `OVERVIEW.md` at `file.ext@/OVERVIEW.md`.
///
/// Resolves lazily via [`FragmentResolver`] — never stale after writes.
pub(in crate::provider::syntax) struct FileOverviewContent {
    pub resolver: FragmentResolver,
    pub filename: String,
    pub language: String,
}

impl TemplateView for FileOverviewContent {
    /// Render the file-level overview with doc text and symbol table.
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let shared = self.resolver.decompose()?;
        Ok(engine.render_bytes(template, &minijinja::context! {
            filename => &self.filename,
            language => &self.language,
            ext => Path::new(&self.filename).extension().and_then(|e| e.to_str()).unwrap_or(""),
            total_lines => shared.rope.line_len(),
            total_bytes => shared.source.len(),
            file_doc => file_doc_text(&shared),
            fragments => fragment_list(&shared.decomposed, &shared),
        }))
    }
}

/// Extract the file-level doc comment as stripped plain text, if present.
#[allow(clippy::redundant_pub_crate)]
pub(crate) fn file_doc_text(shared: &DecomposedSource) -> Option<String> {
    Some(
        shared.decomposer.strip_doc_comment(
            shared.source.get(
                find_fragment_of_kind(&shared.decomposed, &FragmentKind::Docstring)?
                    .span
                    .byte_range
                    .clone(),
            )?,
        ),
    )
}
