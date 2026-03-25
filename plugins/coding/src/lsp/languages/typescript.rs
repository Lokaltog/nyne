use super::prelude::*;
use crate::syntax::languages::typescript::EXTENSIONS;

/// TypeScript LSP server configuration.
struct TypeScriptLsp;

/// LSP spec for TypeScript: typescript-language-server with package.json detection.
impl LspSpec for TypeScriptLsp {
    /// File extensions handled by the TypeScript LSP.
    const EXTENSIONS: &'static [&'static str] = EXTENSIONS;
    /// LSP language identifier for TypeScript.
    const LANGUAGE_ID: &'static str = "typescript";

    /// Returns the typescript-language-server definition for Node.js projects.
    fn servers() -> Vec<LspServerDef> {
        vec![
            LspServerDef::new("tsgo")
                .args(&["--lsp", "--stdio"])
                .detect(|root| root.join("package.json").exists()),
            LspServerDef::new("typescript-language-server")
                .args(&["--stdio"])
                .detect(|root| root.join("package.json").exists()),
        ]
    }
}

register_lsp!(TypeScriptLsp);
