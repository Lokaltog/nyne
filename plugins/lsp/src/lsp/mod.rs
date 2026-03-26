//! LSP client lifecycle, transport, and query abstractions.
//!
//! This module manages the full LSP integration: spawning language server
//! subprocesses, sending JSON-RPC messages over stdio, caching query results,
//! and exposing scoped query handles for both file-level and symbol-level
//! operations. Language servers are built from config-driven definitions
//! and looked up by file extension through [`LspRegistry`].
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

use std::collections::{HashMap, HashSet};

use nyne_source::syntax::SyntaxRegistry;
use spec::LspServerDef;

use crate::config::{LspConfig, ServerEntry};

/// Extension-indexed registry of LSP server definitions.
///
/// Built from the merged config server list at startup. Provides O(1)
/// lookup by file extension.
pub struct LspRegistry {
    /// extension -> list of server definitions (ordered by priority)
    servers: HashMap<String, Vec<LspServerDef>>,
    /// extension -> LSP language identifier (for `textDocument/didOpen`)
    language_ids: HashMap<String, String>,
}

impl LspRegistry {
    /// Build the registry from the merged config server list.
    ///
    /// Server entries may contain duplicates from different config layers
    /// (defaults, user, project) because `deep_merge` concatenates arrays.
    /// Later entries override earlier ones (per field), and disabled
    /// entries are filtered out.
    pub(crate) fn build_with_config(config: &LspConfig) -> Self {
        let syntax = SyntaxRegistry::global();
        let resolved = Self::resolve_servers(&config.servers);

        let mut servers: HashMap<String, Vec<LspServerDef>> = HashMap::new();
        let mut language_ids: HashMap<String, String> = HashMap::new();

        for entry in &resolved {
            if !entry.enabled {
                continue;
            }

            let Some(extensions) = &entry.extensions else {
                tracing::warn!(server = %entry.name, "server has no extensions — skipping");
                continue;
            };

            let def = LspServerDef::from_entry(entry);

            let supported_exts = extensions.iter().filter(|e| syntax.get(e.as_str()).is_some());
            for ext in supported_exts {
                servers.entry(ext.clone()).or_default().push(def.clone());
                let id = entry.language_ids.as_ref().and_then(|ids| ids.resolve(ext));
                if let Some(id) = id {
                    language_ids.entry(ext.clone()).or_insert_with(|| id.to_owned());
                }
            }
        }

        Self { servers, language_ids }
    }

    /// Deduplicate server entries by name. Later entries override earlier ones
    /// per-field (Option fields: last `Some` wins; `enabled`: last value wins).
    fn resolve_servers(entries: &[ServerEntry]) -> Vec<ServerEntry> {
        let mut resolved: Vec<ServerEntry> = Vec::new();
        for entry in entries {
            if let Some(existing) = resolved.iter_mut().find(|e| e.name == entry.name) {
                existing.overlay(entry);
            } else {
                resolved.push(entry.clone());
            }
        }
        resolved
    }

    /// Get server definitions for a file extension.
    pub(crate) fn servers_for(&self, ext: &str) -> &[LspServerDef] { self.servers.get(ext).map_or(&[], Vec::as_slice) }

    /// Get the LSP language identifier for a file extension (for `textDocument/didOpen`).
    pub(crate) fn language_id_for(&self, ext: &str) -> Option<&str> { self.language_ids.get(ext).map(String::as_str) }

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
        self.servers
            .values()
            .flatten()
            .map(LspServerDef::command_str)
            .collect::<HashSet<_>>()
            .into_iter()
    }
}

/// Unit tests.
#[cfg(test)]
mod tests;
