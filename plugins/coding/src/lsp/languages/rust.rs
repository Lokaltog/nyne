use super::prelude::*;
use crate::syntax::languages::rust::EXTENSIONS;

/// Rust LSP server configuration.
struct RustLsp;

/// LSP spec for Rust: rust-analyzer with Cargo.toml detection.
impl LspSpec for RustLsp {
    const EXTENSIONS: &'static [&'static str] = EXTENSIONS;
    const LANGUAGE_ID: &'static str = "rust";

    fn servers() -> Vec<LspServerDef> {
        vec![LspServerDef::new("rust-analyzer").detect(|root| root.join("Cargo.toml").exists())]
    }
}

register_lsp!(RustLsp);
