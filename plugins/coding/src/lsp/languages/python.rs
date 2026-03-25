use super::prelude::*;
use crate::syntax::languages::python::EXTENSIONS;

/// Python LSP server configuration.
struct PythonLsp;

/// LSP spec for Python: basedpyright with pyproject.toml detection.
impl LspSpec for PythonLsp {
    /// File extensions handled by the Python LSP.
    const EXTENSIONS: &'static [&'static str] = EXTENSIONS;

    /// LSP language identifier for Python.
    fn language_id(_ext: &str) -> &'static str { "python" }

    /// Returns the basedpyright server definition for Python projects.
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
