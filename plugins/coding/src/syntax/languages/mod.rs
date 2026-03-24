//! Built-in language decomposers.

/// Common imports shared across language decomposers.
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
