//! Minijinja [`Object`] wrappers for [`DecomposedSource`] and [`Fragment`].
//!
//! These expose fragment trees as rich template objects with property access
//! and method calls — decomposer methods like `clean_doc_comment` are called
//! directly from templates without intermediate Rust flattening.

use std::fmt;
use std::sync::Arc;

use minijinja::value::{Enumerator, Object, ObjectRepr, Value};

use crate::syntax::decomposed::DecomposedSource;

/// Template key for the shared symbol table partial.
pub const SYMBOL_TABLE_PARTIAL_KEY: &str = "syntax/symbol_table";

/// Template source for the shared symbol table partial.
pub const SYMBOL_TABLE_PARTIAL_SRC: &str = include_str!("../../providers/syntax/templates/symbol_table.md.j2");
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
    frag: Fragment,
    shared: Arc<DecomposedSource>,
}

impl fmt::Display for FragmentView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}", self.frag.name) }
}

impl Object for FragmentView {
    fn repr(self: &Arc<Self>) -> ObjectRepr { ObjectRepr::Map }

    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "name" => Some(Value::from(self.frag.name.as_str())),
            "kind" => Some(Value::from(format_kind(&self.frag))),
            "visibility" => Some(Value::from(self.visibility())),
            "signature" => self.frag.signature.as_deref().map(Value::from),
            "line_range" => {
                let lr = self.frag.line_range(&self.shared.source);
                Some(Value::from(format!("{}-{}", lr.start + 1, lr.end)))
            }
            "bytes" => Some(Value::from(self.frag.full_span().len())),
            "children" => Some(fragment_list(&self.frag.children, &self.shared)),
            "child_count" => {
                let count = self
                    .frag
                    .children
                    .iter()
                    .filter(|c| !matches!(c.kind, FragmentKind::CodeBlock { .. }) && !c.kind.is_structural())
                    .count();
                Some(Value::from(count))
            }
            "fs_name" => self.frag.fs_name.as_deref().map(Value::from),
            "code_blocks" => Some(Value::from(code_block_summary(&self.frag.children))),
            "is_code_block" => Some(Value::from(matches!(self.frag.kind, FragmentKind::CodeBlock { .. }))),
            _ => None,
        }
    }

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

impl FragmentView {
    /// Extract the description: doc comment first line, or first content line for sections.
    fn description(&self) -> String {
        if let Some(doc) = self.frag.child_of_kind(&FragmentKind::Docstring) {
            let raw = &self.shared.source[doc.byte_range.clone()];
            return self.shared.decomposer.clean_doc_comment(raw).unwrap_or_default();
        }
        match &self.frag.metadata {
            Some(FragmentMetadata::Document { .. }) => {
                let body = &self.shared.source[self.frag.byte_range.clone()];
                section_first_line(body).unwrap_or_default()
            }
            _ => String::new(),
        }
    }

    fn visibility(&self) -> &str {
        match &self.frag.visibility {
            Some(vis) => compact_visibility(vis),
            None => "",
        }
    }
}

/// Build a minijinja `Value` list of `FragmentView` objects, skipping
/// code blocks and structural fragments (docstrings, imports, decorators).
pub fn fragment_list(fragments: &[Fragment], shared: &Arc<DecomposedSource>) -> Value {
    Value::from(
        fragments
            .iter()
            .filter(|f| !matches!(f.kind, FragmentKind::CodeBlock { .. }) && !f.kind.is_structural())
            .map(|f| {
                Value::from_object(FragmentView {
                    frag: f.clone(),
                    shared: Arc::clone(shared),
                })
            })
            .collect::<Vec<_>>(),
    )
}

/// Format a fragment's kind for display (e.g. "Struct", "h2").
fn format_kind(frag: &Fragment) -> String {
    match &frag.kind {
        FragmentKind::Symbol(k) => k.to_string(),
        FragmentKind::Docstring => "Docstring".into(),
        FragmentKind::Imports => "Imports".into(),
        FragmentKind::Decorator => "Decorator".into(),
        FragmentKind::Section { level } => format!("h{level}"),
        FragmentKind::CodeBlock { lang } => lang
            .as_ref()
            .map_or_else(|| "CodeBlock".into(), |l| format!("CodeBlock({l})")),
        FragmentKind::Preamble => "Preamble".into(),
    }
}

/// Shorten Rust visibility qualifiers for display.
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
    let langs: Vec<&str> = children
        .iter()
        .filter_map(|c| match &c.kind {
            FragmentKind::CodeBlock { lang } => Some(lang.as_deref().unwrap_or("txt")),
            _ => None,
        })
        .collect();

    if langs.is_empty() {
        return String::new();
    }

    let mut unique = langs.clone();
    unique.sort_unstable();
    unique.dedup();
    let count = langs.len();
    let label = if count == 1 { "block" } else { "blocks" };
    format!("{count} {label} ({})", unique.join(", "))
}

/// First non-empty, non-heading line from a markdown section body.
fn section_first_line(body: &str) -> Option<String> {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(String::from)
}

#[cfg(test)]
mod tests;
