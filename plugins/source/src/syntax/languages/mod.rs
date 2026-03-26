//! Built-in language decomposers.
//!
//! Each submodule implements [`LanguageSpec`](super::spec::LanguageSpec) for one
//! language and registers itself via [`register_syntax!`](super::register_syntax).
//! Adding a new language requires: a `LanguageSpec` impl, a `register_syntax!`
//! call, and a tree-sitter grammar dependency. See `prelude` for the shared
//! import set used by all language modules.

/// Common imports shared across language decomposers.
///
/// Re-exports the fragment types, naming strategies, parser utilities, and
/// spec traits that every `LanguageSpec` implementation needs. Language
/// modules use `use super::prelude::*;` to pull in the standard toolkit.
mod prelude;

/// Fennel language decomposer.
#[cfg(feature = "lang-fennel")]
mod fennel;
/// Jinja2 template language decomposer.
#[cfg(feature = "lang-jinja2")]
pub(super) mod jinja2;

/// Markdown language decomposer.
#[cfg(feature = "lang-markdown")]
mod markdown;
/// Nix language decomposer.
#[cfg(feature = "lang-nix")]
mod nix;
/// Python language decomposer.
#[cfg(feature = "lang-python")]
pub mod python;
/// Rust language decomposer.
#[cfg(feature = "lang-rust")]
pub mod rust;
/// TOML language decomposer.
#[cfg(feature = "lang-toml")]
mod toml;
/// TypeScript/JavaScript language decomposer.
#[cfg(feature = "lang-typescript")]
pub mod typescript;
