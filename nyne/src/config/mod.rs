use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use color_eyre::eyre::{Result, WrapErr};
use directories::ProjectDirs;
use garde::Validate;
use rustix::mount::MountFlags;
use serde::{Deserialize, Serialize};

/// Top-level nyne configuration, deserialized from `~/.config/nyne/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct NyneConfig {
    /// Mount configuration (optional — omit if not using FUSE mount).
    #[garde(dive)]
    pub mount: Option<MountConfig>,

    /// Repository configuration — controls how the project is exposed to the daemon.
    #[serde(default)]
    #[garde(skip)]
    pub repository: RepositoryConfig,

    /// Sandbox configuration — controls namespace isolation settings.
    #[serde(default)]
    #[garde(skip)]
    pub sandbox: SandboxConfig,

    /// Agent file configuration (virtual files with module maps).
    #[serde(default)]
    #[garde(dive)]
    pub agent_files: AgentFilesConfig,

    /// Process names that see only the real filesystem (full passthrough).
    ///
    /// Matched against `/proc/{pid}/comm` (auto-truncated to 15 chars).
    /// Defaults to `["git"]`. Plugins may contribute additional entries
    /// at activation time (e.g., LSP servers via [`PassthroughProcesses`]).
    #[garde(skip)]
    #[serde(default = "default_passthrough_processes")]
    pub passthrough_processes: Vec<String>,

    /// Per-plugin configuration sections.
    ///
    /// Each key is a plugin ID (e.g., `"coding"`, `"git"`). Values are
    /// opaque TOML tables — plugins deserialize their own config.
    ///
    /// ```toml
    /// [plugin.coding]
    /// lsp.enabled = true
    /// ```
    #[serde(default)]
    #[garde(skip)]
    pub plugin: HashMap<String, toml::Value>,
}

/// Default implementation for `NyneConfig`.
impl Default for NyneConfig {
    /// Return a config with no mount, default repository/sandbox/agent settings, and no plugins.
    fn default() -> Self {
        Self {
            mount: None,
            repository: RepositoryConfig::default(),
            sandbox: SandboxConfig::default(),
            agent_files: AgentFilesConfig::default(),
            passthrough_processes: default_passthrough_processes(),
            plugin: HashMap::default(),
        }
    }
}

/// Repository configuration — controls how the project is exposed to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct RepositoryConfig {
    /// How the project is exposed to the FUSE daemon.
    ///
    /// See [`StorageStrategy`] for variant descriptions.
    #[serde(default)]
    pub storage_strategy: StorageStrategy,
}

/// Namespace isolation settings for sandboxed subprocesses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxConfig {
    /// Hostname visible inside the sandbox (via UTS namespace).
    ///
    /// Appears in shell prompts as `user@<hostname>`. Set to distinguish
    /// sandboxed shells from regular ones at a glance.
    #[serde(default = "default_sandbox_hostname")]
    pub hostname: String,

    /// Additional bind mounts to expose host directories inside the sandbox.
    ///
    /// Each entry creates a bind mount from a host path to a sandbox path
    /// with configurable mount flags (read-only, noexec, nosuid, nodev).
    #[serde(default)]
    pub bind_mounts: Vec<BindMount>,

    /// Extra environment variables set in sandbox subprocesses (e.g., LSP
    /// servers). These are merged on top of the default propagated set —
    /// use this to inject variables the sandbox wouldn't otherwise see.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Default implementation for `SandboxConfig`.
impl Default for SandboxConfig {
    /// Return sandbox config with default hostname, no bind mounts, and no extra env vars.
    fn default() -> Self {
        Self {
            hostname: default_sandbox_hostname(),
            bind_mounts: Vec::new(),
            env: HashMap::new(),
        }
    }
}

/// Return the default sandbox hostname.
fn default_sandbox_hostname() -> String { "nyne-sandbox".to_owned() }

/// Mount flags for user-configured bind mounts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindMountFlag {
    /// Mount as read-only.
    ReadOnly,
    /// Prevent execution of binaries.
    Noexec,
    /// Ignore setuid/setgid bits.
    Nosuid,
    /// Ignore device special files.
    Nodev,
}

/// Conversion from config-level bind mount flags to kernel mount flags.
impl BindMountFlag {
    /// Convert this config-level flag to the corresponding `rustix` kernel mount flag.
    const fn to_mount_flag(self) -> MountFlags {
        use rustix::mount::MountFlags;

        match self {
            Self::ReadOnly => MountFlags::RDONLY,
            Self::Noexec => MountFlags::NOEXEC,
            Self::Nosuid => MountFlags::NOSUID,
            Self::Nodev => MountFlags::NODEV,
        }
    }
}

/// A user-configured bind mount mapping a host path into the sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindMount {
    /// Absolute path on the host to bind-mount from.
    pub source: PathBuf,

    /// Absolute path inside the sandbox where `source` will appear.
    pub target: PathBuf,

    /// Mount flags to apply (default: none — mount is RW with exec).
    #[serde(default)]
    pub flags: Vec<BindMountFlag>,
}

/// Methods for computing kernel mount flags from config-level flags.
impl BindMount {
    /// Combine all configured flags into a single `MountFlags` bitset, or `None` if no flags are set.
    pub fn mount_flags(&self) -> Option<MountFlags> {
        use rustix::mount::MountFlags;

        let mut flags = MountFlags::empty();
        for flag in &self.flags {
            flags |= flag.to_mount_flag();
        }
        if flags.is_empty() { None } else { Some(flags) }
    }
}

/// Strategy for exposing the project to the FUSE daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, strum::Display)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum StorageStrategy {
    /// Bind-mount the repository directly — no clone, no overlayfs.
    ///
    /// Writes through FUSE hit the real filesystem. The sandbox still
    /// isolates the rest of the host, but the project directory is not
    /// protected from modification.
    #[default]
    Passthrough,
    /// Copy only HEAD tree objects via the git object database.
    ///
    /// Minimal disk usage (~working tree size), works across filesystems.
    /// The clone borrows no state from the source repo at runtime — all
    /// required objects are copied into the target's object store.
    /// Overlayfs captures writes in `~/.cache/nyne/overlay/`.
    Snapshot,
    /// Full `git clone --local` with hardlinked objects.
    ///
    /// Near-zero object store overhead when source and target are on the
    /// same filesystem (hardlinks). Falls back to a full copy when they
    /// are on different filesystems — this can be very large for repos
    /// with extensive history. Overlayfs captures writes.
    Hardlink,
}

/// Configuration for the FUSE virtual mount.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct MountConfig {
    /// Path to the source directory to expose via FUSE.
    #[garde(skip)]
    pub source_dir: PathBuf,

    /// Path where the virtual filesystem will be mounted.
    #[garde(skip)]
    pub mountpoint: PathBuf,

    /// Glob patterns for files/directories to exclude from the virtual mount.
    #[garde(skip)]
    #[serde(default)]
    pub excluded_patterns: Vec<String>,
}

/// Configuration for agent-facing virtual files injected into every directory.
///
/// These files contain a module map of top-level symbols for all source files
/// in the directory, with optional user-authored content from real files.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct AgentFilesConfig {
    /// Filenames to expose as virtual agent files in every directory.
    /// If a real file with the same name exists, its content is prepended.
    #[serde(default = "default_agent_filenames")]
    #[garde(skip)]
    pub filenames: Vec<String>,
}

/// Return the default list of agent-facing virtual filenames.
pub(crate) fn default_agent_filenames() -> Vec<String> { vec!["CLAUDE.md".to_owned(), "AGENTS.md".to_owned()] }

/// Return the default passthrough process list.
fn default_passthrough_processes() -> Vec<String> { vec!["git".to_owned()] }

/// Default implementation for `AgentFilesConfig`.
impl Default for AgentFilesConfig {
    /// Return agent files config with the default filename list.
    fn default() -> Self {
        Self {
            filenames: default_agent_filenames(),
        }
    }
}

/// Return the XDG config file path for nyne.
///
/// Resolves to `~/.config/nyne/config.toml` on Linux.
fn config_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "nyne").map(|dirs| dirs.config_dir().join("config.toml"))
}

/// Loading and validation of the nyne configuration file.
impl NyneConfig {
    /// Load configuration from the XDG config file, falling back to defaults if the file is absent.
    pub fn load() -> Result<Self> {
        let Some(path) = config_path() else {
            tracing::debug!("no XDG config directory found, using defaults");
            return Ok(Self::default());
        };

        tracing::debug!(path = %path.display(), "loading config");

        let config: Self = toml::from_str(&match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                tracing::debug!(path = %path.display(), "config file not found, using defaults");
                return Ok(Self::default());
            }
            Err(e) => {
                return Err(e).wrap_err_with(|| format!("reading {}", path.display()));
            }
        })
        .wrap_err_with(|| format!("parsing {}", path.display()))?;

        config.validate().wrap_err("config validation failed")?;

        Ok(config)
    }
}

/// Unit tests for configuration deserialization and validation.
#[cfg(test)]
mod tests;
