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
use crate::syntax;
use crate::syntax::view::fragment_list;

/// View that renders the OVERVIEW.md template for a file's symbol table.
///
/// Resolves lazily via [`FragmentResolver`] — never stale after writes.
pub(in crate::providers::syntax) struct OverviewContent {
    pub resolver: FragmentResolver,
    pub filename: String,
}

impl TemplateView for OverviewContent {
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let shared = self.resolver.decompose()?;
        let view = minijinja::context! {
            filename => &self.filename,
            file_doc => shared.decomposed.file_doc.as_deref(),
            fragments => fragment_list(&shared.decomposed.fragments, &shared),
        };
        Ok(engine.render_bytes(template, &view))
    }
}

/// View for a per-symbol OVERVIEW.md (lists child symbols).
///
/// Resolves lazily via [`FragmentResolver`] — never stale after writes.
pub(in crate::providers::syntax) struct SymbolOverviewContent {
    pub resolver: FragmentResolver,
    pub fragment_path: Vec<String>,
}

impl TemplateView for SymbolOverviewContent {
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let shared = self.resolver.decompose()?;
        let frag = syntax::require_fragment(&shared.decomposed.fragments, &self.fragment_path)?;
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
pub(in crate::providers::syntax) struct FileOverviewContent {
    pub resolver: FragmentResolver,
    pub filename: String,
    pub language: String,
}

impl TemplateView for FileOverviewContent {
    fn render(&self, engine: &TemplateEngine, template: &str) -> Result<Vec<u8>> {
        let shared = self.resolver.decompose()?;
        let total_lines = shared.source.lines().count();
        let total_bytes = shared.source.len();
        let ext = Path::new(&self.filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let view = minijinja::context! {
            filename => &self.filename,
            language => &self.language,
            ext,
            total_lines,
            total_bytes,
            file_doc => shared.decomposed.file_doc.as_deref(),
            fragments => fragment_list(&shared.decomposed.fragments, &shared),
        };
        Ok(engine.render_bytes(template, &view))
    }
}
