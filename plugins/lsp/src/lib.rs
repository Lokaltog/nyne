//! LSP integration plugin for nyne.
//!
//! Manages LSP server lifecycles, caches query results, and exposes LSP
//! intelligence (hover, references, callers, diagnostics, rename, code
//! actions, workspace symbols) as virtual files in the nyne VFS.

pub(crate) mod context;
pub(crate) mod provider;
pub(crate) mod session;

pub use context::LspContextExt;
pub use session::diagnostic_view::{DiagnosticRow, diagnostics_to_rows};

mod plugin;
