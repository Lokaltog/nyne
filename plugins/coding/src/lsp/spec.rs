use std::path::Path;

/// Detection function: given a project root, returns whether this server
/// is applicable to the project.
pub type DetectFn = fn(&Path) -> bool;

/// Definition of an LSP server — its identity, how to spawn it, and
/// when it applies.
///
/// Constructed via builder methods in `LspSpec::servers()`. Merged with
/// config overrides at registry build time.
///
/// `Clone`: needed because languages with multiple extensions (e.g.,
/// TypeScript: ts + tsx) clone server defs per-extension in the registry.
#[derive(Clone)]
/// Definition of an LSP server.
pub struct LspServerDef {
    /// Unique name for this server (e.g., "rust-analyzer", "biome").
    /// Used as the config key for overrides.
    name: String,
    /// Command to spawn. Defaults to `name` if not overridden.
    command: String,
    /// Default arguments passed to the server process.
    args: Vec<String>,
    /// Project detection function. If `None`, the server is always
    /// considered applicable (useful for config-defined custom servers).
    detect: Option<DetectFn>,
}

/// Builder-style construction and configuration for LSP server definitions.
impl LspServerDef {
    /// Create a new server definition. `name` is both the identifier
    /// and the default command.
    pub(crate) fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            command: name.to_owned(),
            args: Vec::new(),
            detect: None,
        }
    }

    /// Override the command (when it differs from the name).
    pub(crate) fn command(mut self, command: &str) -> Self {
        command.clone_into(&mut self.command);
        self
    }

    /// Set default arguments.
    pub(crate) fn args(mut self, args: &[&str]) -> Self {
        self.args = args.iter().map(|&s| s.to_owned()).collect();
        self
    }

    /// Set default arguments from an owned vec (used by config-defined servers).
    pub(crate) fn args_owned(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    /// Set the project detection function.
    pub(crate) fn detect(mut self, f: DetectFn) -> Self {
        self.detect = Some(f);
        self
    }

    /// Server identifier name.
    pub(crate) fn name(&self) -> &str { &self.name }

    /// Executable command to spawn this server.
    pub(crate) fn command_str(&self) -> &str { &self.command }

    /// Command-line arguments passed to the server on spawn.
    pub(crate) fn args_slice(&self) -> &[String] { &self.args }

    /// Override the command on an existing definition (used by config overrides).
    pub(crate) fn set_command(&mut self, command: String) { self.command = command; }

    /// Override the arguments on an existing definition (used by config overrides).
    pub(crate) fn set_args(&mut self, args: Vec<String>) { self.args = args; }

    /// Check whether this server is applicable for the given project root.
    pub(crate) fn is_applicable(&self, root: &Path) -> bool {
        let Some(f) = self.detect else {
            return true;
        };
        let result = f(root);
        if !result {
            tracing::debug!(
                target: "nyne::lsp",
                server = %self.name,
                root = %root.display(),
                "detection returned false — server not applicable for this project",
            );
        }
        result
    }
}

/// Language-specific LSP knowledge.
///
/// Each supported language implements this trait once to declare its
/// default LSP servers. The `register_lsp!` macro bridges it into the
/// `LspRegistry` via the linkme distributed slice.
///
/// This trait is intentionally separate from `LanguageSpec` (tree-sitter).
/// LSP depends on syntax knowledge; syntax never depends on LSP.
pub trait LspSpec: Send + Sync + 'static {
    /// File extensions this LSP configuration covers.
    /// Must be a subset of the corresponding `LanguageSpec::EXTENSIONS`.
    const EXTENSIONS: &'static [&'static str];

    /// LSP language identifier sent in `textDocument/didOpen`.
    ///
    /// Takes the file extension because some languages need different IDs
    /// per extension (e.g., `"ts"` → `"typescript"`, `"tsx"` →
    /// `"typescriptreact"`). See the LSP specification's
    /// "Text Document Language Identifiers" for canonical values.
    fn language_id(ext: &str) -> &'static str;

    /// Default LSP server definitions for this language.
    ///
    /// Order matters: first server is primary (used for queries when
    /// multiple servers support the same capability). Additional servers
    /// provide supplementary features (e.g., linting, formatting).
    fn servers() -> Vec<LspServerDef>;
}

/// Runtime representation of a language's LSP configuration.
///
/// Bridges the static `LspSpec` trait into a value that the `LspRegistry`
/// can store and iterate. Analogous to how `CodeDecomposer<L>` bridges
/// `LanguageSpec` to `Decomposer`.
pub struct LspLanguageDef {
    pub(crate) extensions: &'static [&'static str],
    pub(crate) language_id: fn(&str) -> &'static str,
    pub(crate) servers: Vec<LspServerDef>,
}

/// Construction from compile-time `LspSpec` implementations.
impl LspLanguageDef {
    /// Build a runtime language definition from a compile-time `LspSpec`.
    pub(crate) fn from_spec<S: LspSpec>() -> Self {
        Self {
            extensions: S::EXTENSIONS,
            language_id: S::language_id,
            servers: S::servers(),
        }
    }
}
