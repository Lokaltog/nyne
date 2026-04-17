//! Environment variables that bridge daemon and attached processes.
//!
//! Injected by `sandbox::command_main` into every process spawned inside a
//! sandbox so CLI commands (`nyne exec`, `nyne list`, `nyne attach`, nested
//! `nyne mount`) can locate the owning daemon without path discovery.

/// Control socket path — points at the daemon's Unix domain socket for IPC.
pub const NYNE_CONTROL_SOCKET_ENV: &str = "NYNE_CONTROL_SOCKET";

/// Session directory path — where nested sessions spawned inside the sandbox
/// live. Ensures nested mounts share the parent daemon's session scope instead
/// of landing in per-namespace buckets.
pub const NYNE_SESSION_DIR_ENV: &str = "NYNE_SESSION_DIR";
