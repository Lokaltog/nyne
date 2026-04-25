//! Shared CLI output primitives.
//!
//! All CLI commands write through [`term()`] and style text with [`style()`].
//! This module is the SSOT for terminal access — no direct `println!` or
//! `console::Term` construction elsewhere in `cli/`.
//!
//! Tabular output uses `comfy-table` via [`new_table()`], which applies the
//! house aesthetic (UTF-8 light borders, dynamic column widths). Cell
//! content styling uses `comfy_table::{Cell, Color, Attribute}` directly
//! rather than `console::style` — both produce ANSI but `comfy_table` needs
//! to know the visible width to pad correctly.

use std::sync::LazyLock;

pub(super) use comfy_table::presets::UTF8_FULL;
pub(super) use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table};
pub(super) use console::{Term, style};

/// Return a stdout terminal handle for CLI output.
///
/// All CLI commands write user-facing output through this function rather
/// than using `println!` or constructing `console::Term` directly. This
/// centralisation means we can swap to stderr, add buffering, or change
/// terminal capabilities in one place without touching every subcommand.
///
/// The returned [`Term`] supports styled output via [`style()`] and is
/// safe to use in non-terminal contexts (e.g., piped output) -- `console`
/// will automatically strip ANSI codes when stdout is not a TTY.
pub(super) fn term() -> &'static Term {
    static TERM: LazyLock<Term> = LazyLock::new(Term::stdout);
    &TERM
}

/// Construct a fresh `comfy_table::Table` configured with the nyne CLI
/// aesthetic: UTF-8 light borders (box-drawing), dynamic column widths.
///
/// Callers set their own header via [`Table::set_header`] and append rows
/// via [`Table::add_row`]. Style cells with `Cell::new(..).fg(Color::..)`
/// and `.add_attribute(Attribute::Bold)` for header emphasis.
pub(super) fn new_table() -> Table {
    let mut t = Table::new();
    t.load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);
    t
}

/// Render a populated table to a string, or a dimmed fallback message when
/// the table has no rows. Centralises the "No X found." idiom across
/// commands so empty-state copy stays consistent. Callers pass the result
/// to [`Term::write_line`].
pub(super) fn render_or_empty(table: &Table, empty_msg: impl AsRef<str>) -> String {
    if table.row_count() == 0 {
        style(empty_msg.as_ref()).dim().to_string()
    } else {
        table.to_string()
    }
}

/// Turn plain header labels into bold `Cell`s — the house header styling
/// for every nyne table. Keeps the bold-attribute incantation in one place.
pub(super) fn bold_headers<const N: usize>(labels: [&str; N]) -> [Cell; N] {
    labels.map(|l| Cell::new(l).add_attribute(Attribute::Bold))
}

#[cfg(test)]
mod tests;
