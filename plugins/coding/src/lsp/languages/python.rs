use super::prelude::*;
use crate::syntax::languages::python::EXTENSIONS;

/// Python LSP server configuration.
///
/// Uses [basedpyright](https://github.com/DetachHead/basedpyright) as the
/// language server. Detection requires `pyproject.toml` in the project root.
struct PythonLsp;

/// LSP spec for Python: basedpyright with pyproject.toml detection.
impl LspSpec for PythonLsp {
    /// File extensions handled by the Python LSP -- reused from the syntax layer.
    const EXTENSIONS: &'static [&'static str] = EXTENSIONS;

    /// LSP language identifier for Python (`"python"` for all extensions).
    fn language_id(_ext: &str) -> &'static str { "python" }

    /// Returns the basedpyright server definition for Python projects.
    ///
    /// The executable is `basedpyright-langserver` (not `basedpyright`, which
    /// is the CLI wrapper). Only activated when `pyproject.toml` exists.
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
