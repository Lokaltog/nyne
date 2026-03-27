//! LSP integration plugin for nyne.
//!
//! Manages LSP server lifecycles, caches query results, and exposes LSP
//! intelligence (hover, references, callers, diagnostics, rename, code
//! actions, workspace symbols) as virtual files in the nyne VFS.

pub mod config;
pub mod providers;
pub mod session;

mod plugin;
