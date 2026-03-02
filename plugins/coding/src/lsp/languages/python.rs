use super::prelude::*;
use crate::syntax::languages::python::EXTENSIONS;

/// Python LSP server configuration.
struct PythonLsp;

impl LspSpec for PythonLsp {
    const EXTENSIONS: &'static [&'static str] = EXTENSIONS;
    const LANGUAGE_ID: &'static str = "python";

    fn servers() -> Vec<LspServerDef> {
        vec![
            LspServerDef::new("basedpyright")
                .command("basedpyright-langserver")
                .args(&["--stdio"])
                .detect(|root| root.join("pyproject.toml").exists()),
        ]
    }
}

register_lsp!(PythonLsp);
