//! Application configuration for the nyne daemon.
//!
//! Deserialized from `~/.config/nyne/config.toml` via [`NyneConfig::load`].
//! All fields have sensible defaults so the config file can be omitted entirely.
//! Plugin-specific sections live in the `plugin` table as opaque TOML values --
//! each plugin deserializes its own section independently.

use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr};
use directories::ProjectDirs;
use garde::Validate;
use rustix::mount::MountFlags;
use serde::{Deserialize, Serialize};

/// Top-level nyne configuration, deserialized from `~/.config/nyne/config.toml`.
///
/// All fields have defaults so a config file is never required. The struct is
/// validated with `garde` after deserialization -- fields marked `#[garde(dive)]`
/// are recursively validated, while `#[garde(skip)]` fields rely on serde's
/// `deny_unknown_fields` for basic correctness.
///
/// Plugin configs are stored as opaque `toml::Value` tables keyed by plugin ID.
/// Each plugin deserializes its own section during activation, which keeps the
/// core config type free of plugin-specific knowledge.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct NyneConfig {
    /// Mount configuration (optional -- omit if not using FUSE mount).
    #[garde(dive)]
    pub mount: Option<MountConfig>,

    /// Repository configuration -- controls how the project is exposed to the daemon.
    #[serde(default)]
    #[garde(skip)]
    pub repository: RepositoryConfig,

    /// Sandbox configuration -- controls namespace isolation settings.
    #[serde(default)]
    #[garde(skip)]
    pub sandbox: SandboxConfig,

    /// Agent file configuration (virtual files with module maps).
    #[serde(default)]
    #[garde(dive)]
    pub agent_files: AgentFilesConfig,

    /// Plugin configuration values, stored as opaque TOML values.
    ///
    /// Each key is a plugin ID (e.g., `"source"`, `"git"`). Values are
    /// opaque TOML tables — plugins deserialize their own config via
    /// [`PluginConfig::from_section`](crate::plugin::PluginConfig::from_section).
    ///
    /// ```toml
    /// [plugin.source]
    /// enabled = true
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
            plugin: HashMap::default(),
        }
    }
}

/// Repository configuration -- controls how the project is exposed to the daemon.
///
/// Currently contains only the storage strategy, but exists as a separate
/// struct so that future repository-level settings (e.g., sparse checkout
/// patterns, submodule handling) have a natural home without widening
/// [`NyneConfig`] itself.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryConfig {
    /// How the project is exposed to the FUSE daemon.
    ///
    /// See [`StorageStrategy`] for variant descriptions.
    #[serde(default)]
    pub storage_strategy: StorageStrategy,
}

/// Namespace isolation settings for sandboxed subprocesses.
///
/// Controls the UTS hostname, bind-mounted directories, and environment
/// variables visible inside the sandbox. These settings affect both the
/// daemon process and any processes spawned via `nyne attach`.
///
/// The sandbox uses Linux namespaces (mount, PID, UTS) for isolation.
/// Bind mounts are the primary mechanism for selectively exposing host
/// directories (e.g., `~/.ssh`, `~/.config`) that the sandboxed process
/// needs but wouldn't otherwise see.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SandboxConfig {
    /// Hostname visible inside the sandbox (via UTS namespace).
    ///
    /// Appears in shell prompts as `user@<hostname>`. Set to distinguish
    /// sandboxed shells from regular ones at a glance.
    pub hostname: String,

    /// Additional bind mounts to expose host directories inside the sandbox.
    ///
    /// Each entry creates a bind mount from a host path to a sandbox path
    /// with configurable mount flags (read-only, noexec, nosuid, nodev).
    pub bind_mounts: Vec<BindMount>,

    /// Extra environment variables set in sandbox subprocesses (e.g., LSP
    /// servers). These are merged on top of the default propagated set --
    /// use this to inject variables the sandbox wouldn't otherwise see.
    pub env: HashMap<String, String>,

    /// Root directory for per-process sandbox state.
    ///
    /// Each running daemon and attach-command child creates a subdirectory
    /// `<state_root>/proc/<pid>/fs/{root,merged,lower,upper,work}` containing
    /// its overlay scaffolding. Removing `<state_root>/proc/<pid>/` reaps
    /// all state for that process.
    ///
    /// Defaults to `/tmp/nyne`.
    pub state_root: PathBuf,

    /// Mount point for the nyne VFS inside the sandbox.
    ///
    /// This is the single path through which the project is accessible to
    /// sandboxed processes. After `pivot_root`, the FUSE filesystem is
    /// attached here and all project access goes through it.
    ///
    /// Defaults to `/code`.
    pub mount_root: PathBuf,
}

/// Default implementation for `SandboxConfig`.
impl Default for SandboxConfig {
    /// Return sandbox config with default hostname, no bind mounts, no extra
    /// env vars, state root `/tmp/nyne`, and mount root `/code`.
    fn default() -> Self {
        Self {
            hostname: "nyne-sandbox".to_owned(),
            bind_mounts: Vec::new(),
            env: HashMap::new(),
            state_root: PathBuf::from("/tmp/nyne"),
            mount_root: PathBuf::from("/code"),
        }
    }
}

/// Mount flags for user-configured bind mounts.
///
/// These are the config-level representations of Linux kernel mount flags.
/// They map 1:1 to `rustix::mount::MountFlags` variants via
/// [`BindMountFlag::to_mount_flag`]. Using a dedicated enum rather than
/// exposing raw kernel flags keeps the config file human-readable and
/// prevents users from setting flags that don't make sense for bind mounts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindMountFlag {
    /// Mount as read-only -- prevents all write operations.
    ReadOnly,
    /// Prevent execution of binaries on this mount.
    Noexec,
    /// Ignore setuid/setgid bits on executables.
    Nosuid,
    /// Ignore device special files (block/char devices).
    Nodev,
}

impl BindMountFlag {
    /// Convert this config-level flag to the corresponding `rustix` kernel mount flag.
    ///
    /// This is a `const fn` so it can be used in static contexts. The mapping
    /// is exhaustive -- adding a new variant to [`BindMountFlag`] requires
    /// adding a corresponding arm here (enforced by the compiler).
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
///
/// Each entry in [`SandboxConfig::bind_mounts`] becomes a `mount --bind`
/// call during sandbox setup. The `source` path must exist on the host;
/// the `target` path is created inside the sandbox if it doesn't exist.
///
/// # Example (TOML)
///
/// ```toml
/// [[sandbox.bind_mounts]]
/// source = "/home/user/.ssh"
/// target = "/home/user/.ssh"
/// flags = ["read_only"]
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindMount {
    /// Absolute path on the host to bind-mount from.
    pub source: PathBuf,

    /// Absolute path inside the sandbox where `source` will appear.
    pub target: PathBuf,

    /// Mount flags to apply (default: none -- mount is RW with exec).
    #[serde(default)]
    pub flags: Vec<BindMountFlag>,
}

impl BindMount {
    /// Combine all configured flags into a single `MountFlags` bitset.
    ///
    /// Returns `None` if no flags are set, which callers can use to skip
    /// the `mount(..., flags)` syscall entirely and use the kernel defaults
    /// (read-write, exec permitted, suid honored, devices allowed).
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, strum::Display, strum::EnumString)]
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
///
/// Only used when the `[mount]` section is present in the config file.
/// When absent, mount parameters come from CLI arguments instead. This
/// struct exists for declarative configuration (e.g., in CI or daemon
/// mode) where the mount should always use the same source and mountpoint.
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
/// These virtual files contain a module map of top-level symbols for all source
/// files in the directory. If a real file with the same name exists on disk,
/// its content is prepended to the generated map -- this lets users author
/// project-specific instructions (e.g., `CLAUDE.md`) that agents see alongside
/// the auto-generated symbol index.
///
/// The default filenames (`CLAUDE.md`, `AGENTS.md`) are chosen for compatibility
/// with popular AI coding agents.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[serde(default, deny_unknown_fields)]
pub struct AgentFilesConfig {
    /// Filenames to expose as virtual agent files in every directory.
    /// If a real file with the same name exists, its content is prepended.
    #[garde(skip)]
    pub filenames: Vec<String>,
}

/// Return the default list of agent-facing virtual filenames.
///
/// These names are chosen for compatibility with popular AI coding agents:
/// `CLAUDE.md` for Anthropic's Claude Code and `AGENTS.md` as a generic
/// convention. Users can override this list in their config file.
pub(crate) fn default_agent_filenames() -> Vec<String> { vec!["CLAUDE.md".to_owned(), "AGENTS.md".to_owned()] }

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
/// Resolves to `~/.config/nyne/config.toml` on Linux (following the XDG
/// Base Directory Specification). Returns `None` if the platform has no
/// concept of a config directory (unlikely on supported targets).
fn config_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", "nyne").map(|dirs| dirs.config_dir().join("config.toml"))
}
/// Project config filenames, checked in order. First match wins.
const PROJECT_CONFIG_FILENAMES: &[&str] = &[".nyne/config.toml", ".nyne.toml", "nyne.toml"];

/// Load project-level configuration from the project root directory.
///
/// Searches for config files in priority order (first match wins).
/// Returns `None` if no project config file exists.
/// Returns an error if a file exists but cannot be read or parsed.
pub(crate) fn load_project_config(project_root: &Path) -> Result<Option<toml::Value>> {
    for filename in PROJECT_CONFIG_FILENAMES {
        let path = project_root.join(filename);
        match fs::read_to_string(&path) {
            Ok(contents) => {
                tracing::debug!(path = %path.display(), "loading project config");
                let value: toml::Value =
                    toml::from_str(&contents).wrap_err_with(|| format!("parsing {}", path.display()))?;
                return Ok(Some(value));
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {}

            Err(e) => return Err(e).wrap_err_with(|| format!("reading {}", path.display())),
        }
    }
    Ok(None)
}
/// Load user configuration from the XDG config file.
///
/// Returns `None` if no XDG directory exists or the config file is missing.
/// Returns an error if the file exists but cannot be read or parsed.
pub(crate) fn load_user_config() -> Result<Option<toml::Value>> {
    let Some(path) = config_path() else {
        tracing::debug!("no XDG config directory found, skipping user config");
        return Ok(None);
    };

    tracing::debug!(path = %path.display(), "loading user config");

    match fs::read_to_string(&path) {
        Ok(contents) => {
            let value: toml::Value =
                toml::from_str(&contents).wrap_err_with(|| format!("parsing {}", path.display()))?;
            Ok(Some(value))
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            tracing::debug!(path = %path.display(), "config file not found, skipping user config");
            Ok(None)
        }
        Err(e) => Err(e).wrap_err_with(|| format!("reading {}", path.display())),
    }
}

/// Unit tests for configuration deserialization and validation.
#[cfg(test)]
mod tests;
