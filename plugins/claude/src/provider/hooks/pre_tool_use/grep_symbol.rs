//! Pre-tool-use Grep symbol-search heuristic.
//!
//! Fires on `Grep` tool calls. Detects patterns that look like symbol
//! searches (qualified paths, method calls, type names, import
//! statements) and suggests nyne's LSP-powered alternatives —
//! `CALLERS.md`, `REFERENCES.md`, `imports.<ext>`.

use std::sync::Arc;

use nyne::prelude::*;
use nyne::templates::TemplateEngine;
use nyne::{Script, ScriptContext};

use crate::provider::hook_schema::GrepToolInput;

const TMPL: &str = "claude/pre-tool-use-grep-symbol";

/// `PreToolUse` Grep symbol-search script.
pub(in crate::provider) struct GrepSymbol {
    pub(in crate::provider) engine: Arc<TemplateEngine>,
}

/// Build the template engine for the [`GrepSymbol`] script.
pub(in crate::provider) fn build_engine() -> Arc<TemplateEngine> {
    let mut b = super::super::hook_builder();
    b.register(TMPL, include_str!("../templates/pre-tool-use/grep-symbol.md.j2"));
    b.finish()
}

/// [`Script`] implementation for [`GrepSymbol`].
impl Script for GrepSymbol {
    /// Detect symbol-search patterns and render LSP-alternative hints.
    fn exec(&self, ctx: &ScriptContext<'_>, stdin: &[u8]) -> Result<Vec<u8>> {
        Ok(super::super::util::run_script(
            ctx,
            stdin,
            &self.engine,
            TMPL,
            "PreToolUse",
            |input, _ctx| {
                let pattern = input.tool_input_as::<GrepToolInput>()?.pattern?;
                let (kind, symbol) = extract_symbol_from_grep(&pattern)?;
                Some(minijinja::context! { kind, symbol })
            },
        ))
    }
}

/// Detect if a grep pattern is searching for symbol usage and extract the symbol name.
///
/// Returns `(kind, symbol)` where kind is "callers", "references", or "imports".
pub(super) fn extract_symbol_from_grep(pattern: &str) -> Option<(&'static str, String)> {
    // Qualified path: Foo::bar → suggest the method (last identifier)
    if pattern.contains("::") {
        let sym = pattern
            .split("::")
            .last()?
            .trim_matches(|c: char| !c.is_alphanumeric() && c != '_');
        if !sym.is_empty() {
            return Some(("callers", sym.to_owned()));
        }
    }

    // Method call: \.method( or fn method
    if pattern.starts_with("\\.") || pattern.starts_with("fn ") || pattern.starts_with("fn\\s") {
        let sym = extract_first_identifier(pattern)?;
        return Some(("callers", sym));
    }

    // Bare function call: word\( or word(
    if pattern.contains("\\(") || (pattern.contains('(') && pattern.chars().next().is_some_and(char::is_alphabetic)) {
        let sym = extract_first_identifier(pattern)?;
        return Some(("callers", sym));
    }

    // PascalCase type name (all alphanumeric, starts with uppercase)
    if pattern.starts_with(|c: char| c.is_ascii_uppercase()) && pattern.chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        return Some(("references", pattern.to_owned()));
    }

    // Import statement
    if pattern.starts_with("use ") || pattern.starts_with("import ") || pattern.starts_with("from ") {
        return Some(("imports", String::new()));
    }

    None
}

/// Extract the first word-like identifier from a pattern, skipping regex syntax.
pub(super) fn extract_first_identifier(pattern: &str) -> Option<String> {
    let start = pattern.find(|c: char| c.is_alphanumeric() || c == '_')?;
    let rest = &pattern[start..];
    let ident: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if ident == "fn" {
        return extract_first_identifier(&rest[ident.len()..]);
    }
    if ident.is_empty() { None } else { Some(ident) }
}
