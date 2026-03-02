//! Shared CLI output primitives.
//!
//! All CLI commands write through [`term()`] and style text with [`style()`].
//! This module is the SSOT for terminal access — no direct `println!` or
//! `console::Term` construction elsewhere in `cli/`.

pub use console::{Style, Term, style};

/// Return a stdout terminal handle.
///
/// All CLI output goes through this — centralised so we can swap to
/// stderr or add buffering in one place.
pub fn term() -> Term { Term::stdout() }
