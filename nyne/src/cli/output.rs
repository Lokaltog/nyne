//! Shared CLI output primitives.
//!
//! All CLI commands write through [`term()`] and style text with [`style()`].
//! This module is the SSOT for terminal access — no direct `println!` or
//! `console::Term` construction elsewhere in `cli/`.

use std::sync::LazyLock;

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
