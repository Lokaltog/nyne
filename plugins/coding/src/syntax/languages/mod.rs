//! Built-in language decomposers.

mod prelude;

#[cfg(feature = "lang-fennel")]
mod fennel;
#[cfg(feature = "lang-jinja2")]
pub(super) mod jinja2;

#[cfg(feature = "lang-markdown")]
mod markdown;
#[cfg(feature = "lang-nix")]
mod nix;
#[cfg(feature = "lang-python")]
pub mod python;
#[cfg(feature = "lang-rust")]
pub mod rust;
#[cfg(feature = "lang-toml")]
mod toml;
#[cfg(feature = "lang-typescript")]
pub mod typescript;
