use super::prelude::*;
use crate::syntax::languages::typescript::EXTENSIONS;

/// TypeScript LSP server configuration.
///
/// Supports both `.ts` and `.tsx` extensions with distinct language
/// identifiers. Two server candidates are registered: `tsgo` (preferred,
/// the native Go port) and `typescript-language-server` (fallback). Both
/// require `package.json` in the project root for activation.
struct TypeScriptLsp;

/// LSP spec for TypeScript: tsgo (preferred) or typescript-language-server
/// with `package.json` detection.
impl LspSpec for TypeScriptLsp {
    /// File extensions handled by the TypeScript LSP -- reused from the syntax layer.
    const EXTENSIONS: &'static [&'static str] = EXTENSIONS;

    /// LSP language identifier: `"typescript"` for `.ts`, `"typescriptreact"` for `.tsx`.
    ///
    /// The LSP spec requires distinct language IDs for TSX because servers
    /// may enable JSX-specific diagnostics and completions only for that ID.
    fn language_id(ext: &str) -> &'static str {
        match ext {
            "tsx" => "typescriptreact",
            _ => "typescript",
        }
    }

    /// Returns TypeScript server definitions for Node.js projects.
    ///
    /// Order matters: `tsgo` (the native Go port of the TS server) is
    /// preferred for its speed. `typescript-language-server` is the
    /// fallback. The first applicable server wins via [`LspManager::client_for_ext`].
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
