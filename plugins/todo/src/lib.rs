//! nyne-todo — TODO/FIXME comment aggregation for nyne.
//!
//! This crate provides the "todo" plugin: scans source files for TODO/FIXME
//! markers and exposes them as virtual files in the nyne VFS.

/// TODO/FIXME provider — scanning, indexing, and VFS exposure.
pub(crate) mod provider;

/// Plugin configuration types.
pub(crate) mod config;

/// Plugin registration and lifecycle implementation.
mod plugin;
