//! nyne-todo — TODO/FIXME comment aggregation for nyne.
//!
//! This crate provides the "todo" plugin: scans source files for TODO/FIXME
//! markers and exposes them as virtual files in the nyne VFS.

/// Extension trait for accessing todo services from `ActivationContext`.
pub(crate) mod context;
/// TODO/FIXME provider — scanning, indexing, and VFS exposure.
pub(crate) mod provider;

/// Plugin registration and lifecycle implementation.
mod plugin;
