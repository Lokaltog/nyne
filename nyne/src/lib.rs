//! Nyne — expose source code as a FUSE filesystem.
//!
//! This is the core library crate for nyne. It provides the FUSE filesystem layer,
//! the request dispatch pipeline (routing, caching, content resolution), the virtual
//! node abstraction, and the plugin/provider system that lets external crates contribute
//! filesystem content.
//!
//! # Architecture
//!
//! The crate is organized into tiers to enforce a clean dependency direction:
//!
//! - **Foundation** (`types`, `text`, `config`, `session`, `process`) — no crate-internal
//!   imports, pure domain types and utilities.
//! - **Infrastructure** (`router`) — middleware pipeline: chain, provider trait, node
//!   capabilities, route tree, filesystem abstraction. No crate-internal imports.
//! - **Domain** (`node`, `err`) — virtual node types and error handling.
//! - **Contracts** (`provider`, `templates`) — the [`Provider`] trait and template rendering
//!   that plugin crates implement.
//! - **Orchestration** (`dispatch`, `fuse`, `watcher`, `sandbox`, `cli`) — ties everything
//!   together into a running FUSE daemon.

/// Middleware pipeline for FUSE operation dispatch.
pub mod router;

/// Error utilities for FUSE errno handling and eyre integration.
pub mod err;

/// Command-line interface: argument parsing and subcommand dispatch.
pub mod cli;
/// Configuration loading and validation from TOML.
pub mod config;
/// Generic deep-merge for layered configuration values.
pub mod deep_merge;
/// Request dispatch: routing, caching, and content pipeline.
pub mod dispatch;
/// FUSE filesystem implementation bridging kernel requests to the dispatch layer.
pub(crate) mod fuse;
/// Path utilities for virtual filesystem operations.
pub mod path_utils;

/// Gitignore-backed path filter for bypassing virtual-content decoration
/// on ignored paths.
pub mod path_filter;
/// Plugin registration and lifecycle management.
pub mod plugin;
/// Common re-exports for convenient use across the crate.
pub mod prelude;
/// Process spawning utilities for subprocess lifecycle management.
pub mod process;
/// Linux namespace sandbox for isolating daemon subprocesses.
pub(crate) mod sandbox;
/// Session management: mount lifecycle and control socket handling.
pub(crate) mod session;
/// MiniJinja template rendering for virtual file content.
pub mod templates;
/// Shared text utilities: slugification, date formatting, diffs.
pub mod text;
/// Shared domain types: VFS paths, file metadata, and identifiers.
pub mod types;
/// Filesystem watcher for real-FS change detection and cache invalidation.
pub(crate) mod watcher;

/// Shared test fixtures, stubs, and helpers.
///
/// Not `#[cfg(test)]` — plugin crates depend on these utilities for their
/// own test code and `#[cfg(test)]` items in a library are invisible to
/// downstream crates even during `cargo test`.
pub mod test_support;

// Public API: only what external consumers (CLI, provider authors) need.
pub use dispatch::ScriptRegistry;
pub use dispatch::activation::ActivationContext;
pub use dispatch::script::{Script, ScriptContext, ScriptEntry, provider_script_address};
pub use fuse::notify::{AsyncNotifier, FuseNotifier, KernelNotifier};
pub use plugin::control::{ControlCommand, ControlContext};
pub use plugin::{PLUGINS, Plugin};
pub use sandbox::{ClonerFactory, PROJECT_CLONERS, ProjectCloner};
pub use types::slice::{SliceSpec, parse_slice_suffix, parse_spec};
pub use types::{ExtensionCounts, FileKind, SymbolLineRange};
