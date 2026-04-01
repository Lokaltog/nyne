//! nyne-nyne — meta-information provider for nyne mounts.
//!
//! Serves `/.nyne.md` at the mount root with live mount status: source
//! directory, detected languages, active providers grouped by plugin, and
//! uptime.

/// Plugin registration and lifecycle.
mod plugin;

/// Provider serving `/.nyne.md` at mount root.
mod provider;
