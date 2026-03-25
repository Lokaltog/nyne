// LSP module is staged infrastructure — providers will consume these types
// in the next phase. All dead-code warnings are from not-yet-wired consumers.
#![allow(dead_code)]

/// TTL-based cache for LSP query results.
pub mod cache;
/// LSP client for communicating with language server subprocesses.
pub mod client;
/// Storage for push diagnostics received from LSP servers.
pub mod diagnostic_store;
/// Template-ready rendering of LSP diagnostics.
pub mod diagnostic_view;
/// Application of LSP workspace edits to files on disk.
pub mod edit;
/// Per-file and per-symbol LSP query handles.
pub mod handle;
/// LSP server lifecycle management and cached queries.
pub mod manager;
/// Path rewriting between FUSE display root and overlay storage root.
pub mod path;
/// Scoped LSP query handles for file-level operations.
pub mod query;
/// LSP server definition traits and types.
pub mod spec;
/// JSON-RPC transport layer for LSP communication.
pub mod transport;
/// Conversions between filesystem paths and LSP URIs.
pub mod uri;

/// Per-language LSP server configurations registered via `register_lsp!`.
mod languages;

use std::collections::{HashMap, HashSet};

use spec::{LspLanguageDef, LspServerDef};

use crate::config::lsp::{CustomLspServer, LspConfig, LspServerOverride};

/// Factory function that creates LSP language definitions for link-time
/// auto-discovery via `linkme`.
pub type LspFactory = fn() -> LspLanguageDef;

/// Link-time registry of LSP language definitions.
#[linkme::distributed_slice]
pub(crate) static LSP_FACTORIES: [LspFactory];

/// Register a language's LSP configuration for link-time auto-discovery.
///
/// # Examples
///
/// ```ignore
/// register_lsp!(RustLsp);
/// ```
macro_rules! register_lsp {
    ($spec:ident) => {
        const _: () = {
            #[allow(unsafe_code)]
            #[linkme::distributed_slice($crate::lsp::LSP_FACTORIES)]
            static LSP_FACTORY: $crate::lsp::LspFactory = || $crate::lsp::spec::LspLanguageDef::from_spec::<$spec>();
        };
    };
}
pub(crate) use register_lsp;

use crate::syntax::SyntaxRegistry;

/// Extension-indexed registry of LSP server definitions.
///
/// Built from the `linkme` distributed slice + config overrides at startup.
/// Provides O(1) lookup by file extension.
///
/// Uses `String` keys (not `&'static str`) to support custom servers
/// defined in config with arbitrary extension strings.
pub struct LspRegistry {
    /// extension -> list of server definitions (ordered by priority)
    servers: HashMap<String, Vec<LspServerDef>>,
    /// extension -> LSP language identifier (for `textDocument/didOpen`)
    language_ids: HashMap<String, &'static str>,
}

/// Registry of LSP language definitions and server configurations.
impl LspRegistry {
    /// Build the registry from all link-time registered LSP language definitions.
    pub(crate) fn build() -> Self {
        let syntax = SyntaxRegistry::global();
        let mut servers: HashMap<String, Vec<LspServerDef>> = HashMap::new();
        let mut language_ids: HashMap<String, &'static str> = HashMap::new();
        for factory in LSP_FACTORIES {
            let lang_def = factory();
            for &ext in lang_def.extensions {
                register_ext(&syntax, ext, &lang_def, &mut servers, &mut language_ids);
            }
        }
        Self { servers, language_ids }
    }

    /// Build with config overrides applied on top of built-in defaults.
    pub(crate) fn build_with_config(config: &LspConfig) -> Self {
        let mut registry = Self::build();
        registry.apply_config(config);
        registry
    }

    /// Apply config overrides: disable servers, change args, add custom servers.
    fn apply_config(&mut self, config: &LspConfig) {
        for (name, override_cfg) in &config.servers {
            if override_cfg.enabled {
                self.override_server(name, override_cfg);
            } else {
                self.remove_server(name);
            }
        }
        for custom in &config.custom {
            self.add_custom_server(custom);
        }
    }

    /// Remove a server by name from all extensions.
    fn remove_server(&mut self, name: &str) {
        for servers in self.servers.values_mut() {
            servers.retain(|s| s.name() != name);
        }
    }

    /// Apply command/args overrides to a server by name across all extensions.
    fn override_server(&mut self, name: &str, cfg: &LspServerOverride) {
        let matching = self
            .servers
            .values_mut()
            .flat_map(|servers| servers.iter_mut())
            .filter(|s| s.name() == name);
        for server in matching {
            if let Some(cmd) = &cfg.command {
                server.set_command(cmd.clone());
            }
            if let Some(args) = &cfg.args {
                server.set_args(args.clone());
            }
        }
    }

    /// Add a config-defined custom server to the registry.
    fn add_custom_server(&mut self, custom: &CustomLspServer) {
        let def = LspServerDef::new(&custom.name)
            .command(&custom.command)
            .args_owned(custom.args.clone());
        for ext in &custom.extensions {
            self.servers.entry(ext.clone()).or_default().push(def.clone());
        }
    }

    /// Get server definitions for a file extension.
    pub(crate) fn servers_for(&self, ext: &str) -> &[LspServerDef] { self.servers.get(ext).map_or(&[], Vec::as_slice) }

    /// Get the LSP language identifier for a file extension (for `textDocument/didOpen`).
    pub(crate) fn language_id_for(&self, ext: &str) -> Option<&str> { self.language_ids.get(ext).copied() }

    /// Return all extensions that have at least one LSP server registered.
    pub(crate) fn extensions(&self) -> Vec<&str> {
        let mut exts: Vec<_> = self.servers.keys().map(String::as_str).collect();
        exts.sort_unstable();
        exts
    }

    /// Return the unique set of server command names across all extensions.
    ///
    /// Used to build the passthrough process set — these processes must see
    /// only the real filesystem so they index the actual source, not virtual
    /// content.
    pub(crate) fn server_commands(&self) -> impl Iterator<Item = &str> {
        let mut seen = HashSet::new();
        self.servers
            .values()
            .flatten()
            .map(LspServerDef::command_str)
            .filter(move |cmd| seen.insert(*cmd))
            .collect::<Vec<_>>()
            .into_iter()
    }
}

/// Register a single file extension's LSP servers into the registry maps.
fn register_ext(
    syntax: &SyntaxRegistry,
    ext: &str,
    lang_def: &LspLanguageDef,
    servers: &mut HashMap<String, Vec<LspServerDef>>,
    language_ids: &mut HashMap<String, &'static str>,
) {
    // Enforce SSOT: every LSP extension must have a syntax
    // (tree-sitter) registration. LSP is gated on syntax —
    // if someone adds an LSP spec for an extension without a
    // corresponding syntax spec, that's a bug.
    assert!(
        syntax.get(ext).is_some(),
        "LSP extension \"{ext}\" has no syntax (tree-sitter) registration — \
         LspSpec::EXTENSIONS must be a subset of LanguageSpec::EXTENSIONS"
    );

    let entry = servers.entry(ext.to_owned()).or_default();

    if !entry.is_empty() {
        warn_duplicate_lsp_ext(ext, entry, lang_def);
    }

    entry.extend(lang_def.servers.iter().cloned());
    language_ids.insert(ext.to_owned(), (lang_def.language_id)(ext));
}

/// Emit a warning when multiple LSP specs register the same file extension.
fn warn_duplicate_lsp_ext(ext: &str, existing: &[LspServerDef], lang_def: &LspLanguageDef) {
    tracing::warn!(
        target: "nyne::lsp",
        ext,
        existing_servers = ?existing.iter().map(LspServerDef::name).collect::<Vec<_>>(),
        new_servers = ?lang_def.servers.iter().map(LspServerDef::name).collect::<Vec<_>>(),
        "multiple LSP specs register the same extension — servers will be merged",
    );
}

/// Unit tests.
#[cfg(test)]
mod tests;
