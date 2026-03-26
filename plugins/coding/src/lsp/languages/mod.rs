//! Per-language LSP server definitions.
//!
//! Each submodule implements [`LspSpec`](super::spec::LspSpec) for a single
//! language and registers itself via [`register_lsp!`](super::register_lsp).
//! Adding a new language requires one file here plus a `mod` declaration --
//! no other changes needed.

/// Common imports for language LSP spec implementations.
mod prelude;

/// Python LSP configuration (basedpyright).
mod python;
/// Rust LSP configuration (rust-analyzer).
mod rust;
/// TypeScript LSP configuration (typescript-language-server).
mod typescript;
