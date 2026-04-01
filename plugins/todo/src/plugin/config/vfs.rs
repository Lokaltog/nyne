use serde::{Deserialize, Serialize};

/// VFS path configuration for the todo plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Vfs {
    /// Top-level todo directory name inside companion directories.
    pub todo: String,

    /// Overview file name inside the todo directory.
    pub overview: String,
}

impl Default for Vfs {
    fn default() -> Self {
        Self {
            todo: "todo".into(),
            overview: "OVERVIEW.md".into(),
        }
    }
}
