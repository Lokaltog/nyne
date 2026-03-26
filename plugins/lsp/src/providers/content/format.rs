//! Formatting helpers for LSP data types.
//!
//! Pure extraction and label functions that convert LSP protocol types
//! (hover contents, inlay hints, symbol kinds) into plain text for
//! template rendering.

use lsp_types::{HoverContents, InlayHintLabel, MarkedString};

/// Extract display text from hover contents.
pub(super) fn extract_hover_content(contents: &HoverContents) -> String {
    match contents {
        HoverContents::Scalar(value) => marked_string_to_text(value),
        HoverContents::Array(values) => values
            .iter()
            .map(marked_string_to_text)
            .collect::<Vec<_>>()
            .join("\n\n"),
        HoverContents::Markup(markup) => markup.value.clone(),
    }
}

/// Convert a `MarkedString` to plain text.
pub(super) fn marked_string_to_text(ms: &MarkedString) -> String {
    match ms {
        MarkedString::String(s) => s.clone(),
        MarkedString::LanguageString(ls) => format!("```{}\n{}\n```", ls.language, ls.value),
    }
}

/// Extract a display string from an inlay hint label.
pub(super) fn extract_inlay_label(label: &InlayHintLabel) -> String {
    match label {
        InlayHintLabel::String(s) => s.clone(),
        InlayHintLabel::LabelParts(parts) => parts.iter().map(|p| p.value.as_str()).collect(),
    }
}
