use super::prelude::*;
use crate::syntax::languages::typescript::EXTENSIONS;

/// TypeScript LSP server configuration.
struct TypeScriptLsp;

/// LSP spec for TypeScript: typescript-language-server with package.json detection.
impl LspSpec for TypeScriptLsp {
    const EXTENSIONS: &'static [&'static str] = EXTENSIONS;
    const LANGUAGE_ID: &'static str = "typescript";

    fn servers() -> Vec<LspServerDef> {
        vec![
            LspServerDef::new("typescript-language-server")
                .args(&["--stdio"])
                .detect(|root| root.join("package.json").exists()),
        ]
    }
}

register_lsp!(TypeScriptLsp);
