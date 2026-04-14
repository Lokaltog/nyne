//! Minijinja [`Object`] wrappers for [`DecomposedSource`] and [`Fragment`].
//!
//! These expose fragment trees as rich template objects with property access
//! and method calls — decomposer methods like `clean_doc_comment` are called
//! directly from templates without intermediate Rust flattening.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use minijinja::value::{Enumerator, Object, ObjectRepr, Value};

use crate::syntax::decomposed::DecomposedSource;

/// Template key for the shared symbol table partial.
pub const SYMBOL_TABLE_PARTIAL_KEY: &str = "syntax/symbol_table";

/// Template source for the shared symbol table partial.
pub const SYMBOL_TABLE_PARTIAL_SRC: &str = include_str!("../../provider/syntax/templates/symbol_table.md.j2");
use crate::syntax::fragment::{Fragment, FragmentKind, FragmentMetadata};

/// Template-accessible wrapper around a [`Fragment`].
///
/// Properties: `name`, `kind`, `visibility`, `signature`, `line_range`,
/// `bytes`, `children`, `fs_name`, `code_blocks`
///
/// Methods: `description()` — doc comment first line (via decomposer)
/// with fallback to signature.
#[derive(Debug)]
pub struct FragmentView {
    fragment: Fragment,
    shared: Arc<DecomposedSource>,
}

/// Display implementation for `FragmentView`, showing the fragment name.
impl fmt::Display for FragmentView {
    /// Displays the fragment name.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}", self.fragment.name) }
}

/// Minijinja [`Object`] implementation exposing fragment properties to templates.
impl Object for FragmentView {
    /// Returns the string representation for minijinja.
    fn repr(self: &Arc<Self>) -> ObjectRepr { ObjectRepr::Map }

    /// Looks up a template variable by key.
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "name" => Some(Value::from(self.fragment.name.as_str())),
            "kind" => Some(Value::from(self.fragment.kind.short_display())),
            "visibility" => Some(Value::from(self.visibility())),
            "signature" => self.fragment.signature.as_deref().map(Value::from),
            "line_range" => {
                let lr = self.fragment.line_range(&self.shared.rope);
                Some(Value::from(format!("{}-{}", lr.start + 1, lr.end)))
            }
            "bytes" => Some(Value::from(self.fragment.span.full_span.len())),
            "children" => Some(fragment_list(&self.fragment.children, &self.shared)),
            "child_count" => Some(Value::from(
                self.fragment
                    .children
                    .iter()
                    .filter(|c| !matches!(c.kind, FragmentKind::CodeBlock { .. }) && !c.kind.is_structural())
                    .count(),
            )),
            "fs_name" => self.fragment.fs_name.as_deref().map(Value::from),
            "code_blocks" => Some(Value::from(code_block_summary(&self.fragment.children))),
            "is_code_block" => Some(Value::from(matches!(
                self.fragment.kind,
                FragmentKind::CodeBlock { .. }
            ))),
            _ => None,
        }
    }

    /// Enumerates available template variable keys.
    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Str(&[
            "name",
            "kind",
            "visibility",
            "signature",
            "line_range",
            "bytes",
            "children",
            "child_count",
            "fs_name",
            "code_blocks",
            "is_code_block",
        ])
    }

    /// Dispatches template method calls.
    fn call_method(
        self: &Arc<Self>,
        _state: &minijinja::State<'_, '_>,
        method: &str,
        _args: &[Value],
    ) -> Result<Value, minijinja::Error> {
        match method {
            "description" => Ok(Value::from(self.description())),
            _ => Err(minijinja::Error::from(minijinja::ErrorKind::UnknownMethod)),
        }
    }
}

/// Helper methods for extracting display data from a fragment.
impl FragmentView {
    /// Extract the description: doc comment first line, or first content line for sections.
    fn description(&self) -> String {
        if let Some(doc) = self.fragment.child_of_kind(&FragmentKind::Docstring) {
            return self
                .shared
                .decomposer
                .clean_doc_comment(&self.shared.source[doc.span.byte_range.clone()])
                .unwrap_or_default();
        }
        match &self.fragment.metadata {
            Some(FragmentMetadata::Document { .. }) =>
                section_first_line(&self.shared.source[self.fragment.span.byte_range.clone()]).unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Returns the compact visibility string.
    fn visibility(&self) -> &str {
        match &self.fragment.visibility {
            Some(vis) => compact_visibility(vis),
            None => "",
        }
    }
}

/// Build a minijinja `Value` list of `FragmentView` objects.
///
/// Skips code blocks, structural fragments (docstrings, imports, decorators),
/// and fragments without a filesystem name (hidden by conflict resolution).
pub fn fragment_list(fragments: &[Fragment], shared: &Arc<DecomposedSource>) -> Value {
    Value::from(
        fragments
            .iter()
            .filter(|f| {
                f.fs_name.is_some() && !matches!(f.kind, FragmentKind::CodeBlock { .. }) && !f.kind.is_structural()
            })
            .map(|f| {
                Value::from_object(FragmentView {
                    fragment: f.clone(),
                    shared: Arc::clone(shared),
                })
            })
            .collect::<Vec<_>>(),
    )
}

/// Shorten Rust visibility qualifiers for display.
///
/// Renders `pub(crate)` as `crate`, `pub(super)` as `super`, etc.
/// Used in OVERVIEW.md symbol tables where column width is limited.
fn compact_visibility(vis: &str) -> &str {
    match vis {
        "pub" => "pub",
        "pub(crate)" => "crate",
        "pub(super)" => "super",
        v if v.starts_with("pub(in ") => "pub",
        _ => vis,
    }
}

/// Summarize code blocks among children (e.g. "2 blocks (rust, sh)").
fn code_block_summary(children: &[Fragment]) -> String {
    let mut count = 0usize;
    let mut unique: BTreeSet<&str> = BTreeSet::new();

    for c in children {
        if let FragmentKind::CodeBlock { lang } = &c.kind {
            count += 1;
            unique.insert(lang.as_deref().unwrap_or("txt"));
        }
    }

    if count == 0 {
        return String::new();
    }

    let label = if count == 1 { "block" } else { "blocks" };
    format!(
        "{count} {label} ({})",
        unique.into_iter().collect::<Vec<_>>().join(", ")
    )
}

/// First non-empty, non-heading line from a markdown section body.
///
/// Used as the description column in OVERVIEW.md for document sections,
/// where the heading itself is already the section name.
fn section_first_line(body: &str) -> Option<String> {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(String::from)
}

/// Tests for fragment view template rendering.
#[cfg(test)]
mod tests;
