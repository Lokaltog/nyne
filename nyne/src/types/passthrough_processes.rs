/// Additional passthrough process names contributed by plugins at activation time.
///
/// Inserted into the [`TypeMap`](super::TypeMap) by plugins (e.g., the coding
/// plugin adds LSP server commands). Core merges these with the config-defined
/// [`passthrough_processes`](crate::config::NyneConfig::passthrough_processes)
/// when building the FUSE handler.
///
/// Core never imports plugin crates — it only reads this core-defined type.
#[derive(Debug, Clone, Default)]
pub struct PassthroughProcesses(pub Vec<String>);
