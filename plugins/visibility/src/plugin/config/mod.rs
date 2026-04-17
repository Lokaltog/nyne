//! Configuration for `[plugin.visibility]`.

use nyne::plugin::PluginConfig;
use serde::{Deserialize, Serialize};

/// Visibility plugin configuration.
///
/// ```toml
/// [plugin.visibility]
/// passthrough_processes = ["git", "rust-analyzer"]
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VisibilityConfig {
    /// Process names that see only the real filesystem (full passthrough).
    ///
    /// Matched against `/proc/{pid}/comm` (auto-truncated to 15 chars).
    /// Defaults to `["git"]`. Other plugins may contribute additional entries
    /// at activation time via [`PassthroughProcesses`](crate::PassthroughProcesses).
    pub passthrough_processes: Vec<String>,
}

impl Default for VisibilityConfig {
    fn default() -> Self {
        Self {
            passthrough_processes: vec!["git".to_owned()],
        }
    }
}

impl PluginConfig for VisibilityConfig {}
