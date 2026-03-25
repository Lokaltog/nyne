use super::prelude::*;
use crate::syntax::languages::rust::EXTENSIONS;

/// Rust LSP server configuration.
struct RustLsp;

/// LSP spec for Rust: rust-analyzer with Cargo.toml detection.
impl LspSpec for RustLsp {
    /// File extensions handled by the Rust LSP.
    const EXTENSIONS: &'static [&'static str] = EXTENSIONS;

    /// LSP language identifier for Rust.
    fn language_id(_ext: &str) -> &'static str { "rust" }

    /// Returns the rust-analyzer server definition for Cargo projects.
    fn servers() -> Vec<LspServerDef> {
        vec![LspServerDef::new("rust-analyzer").detect(|root| root.join("Cargo.toml").exists())]
    }
}

register_lsp!(RustLsp);
