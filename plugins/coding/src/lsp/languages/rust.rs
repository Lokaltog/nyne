use super::prelude::*;
use crate::syntax::languages::rust::EXTENSIONS;

/// Rust LSP server configuration.
///
/// Uses `rust-analyzer` as the language server. Detection requires
/// `Cargo.toml` in the project root.
struct RustLsp;

/// LSP spec for Rust: rust-analyzer with Cargo.toml detection.
impl LspSpec for RustLsp {
    /// File extensions handled by the Rust LSP -- reused from the syntax layer.
    const EXTENSIONS: &'static [&'static str] = EXTENSIONS;

    /// LSP language identifier for Rust (`"rust"` for all extensions).
    fn language_id(_ext: &str) -> &'static str { "rust" }

    /// Returns the rust-analyzer server definition for Cargo projects.
    ///
    /// The server name doubles as the command name (`rust-analyzer`).
    /// Only activated when `Cargo.toml` exists in the project root.
    fn servers() -> Vec<LspServerDef> {
        vec![LspServerDef::new("rust-analyzer").detect(|root| root.join("Cargo.toml").exists())]
    }
}

register_lsp!(RustLsp);
