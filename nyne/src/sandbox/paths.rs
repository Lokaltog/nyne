//! Well-known path constructors for sandbox operations.
//!
//! SSOT for all path construction in the sandbox module. No other
//! sandbox submodule uses `format!` or inline `join` chains for
//! well-known paths — they call functions here instead.
//!
//! Session/socket paths live in `crate::session` — not here.

use std::path::{Path, PathBuf};

use rustix::process::Pid;

/// Static procfs paths (no allocation needed).
pub const UID_MAP: &str = "/proc/self/uid_map";
pub const GID_MAP: &str = "/proc/self/gid_map";
pub const SETGROUPS: &str = "/proc/self/setgroups";

/// `/proc/<pid>/ns/user`
pub fn ns_user(pid: Pid) -> PathBuf { proc_pid(pid).join("ns/user") }

/// `/proc/<pid>/ns/mnt`
pub fn ns_mnt(pid: Pid) -> PathBuf { proc_pid(pid).join("ns/mnt") }

fn proc_pid(pid: Pid) -> PathBuf { Path::new("/proc").join(pid.to_string()) }

/// Strip the leading `/` to make an absolute path relative to some root.
///
/// Returns the path unchanged if it has no root prefix.
pub fn relative_to_root(path: &Path) -> &Path { path.strip_prefix("/").unwrap_or(path) }

/// Temporary root for `pivot_root`: `/tmp/nyne-root-<pid>`.
pub fn newroot(pid: Pid) -> PathBuf { PathBuf::from(format!("/tmp/nyne-root-{pid}")) }

/// Fixed mount point for the FUSE filesystem inside the sandbox.
///
/// All project access goes through this single path — the overlayfs
/// internals are not exposed.
pub const SANDBOX_CODE: &str = "/code";

/// Directory for the nyne binary inside the sandbox.
///
/// The invoking nyne binary is bind-mounted here so the sandbox always
/// uses the same binary that started the session — not whatever version
/// happens to be installed on the host `PATH`.
pub const NYNE_BIN_DIR: &str = "/nyne/bin";

/// Per-PID subdirectory under the persist root for overlay upperdirs.
pub fn persist_slot(persist_root: &Path, pid: Pid) -> PathBuf { persist_root.join(format!("upper-{pid}")) }

/// Default persist root under XDG cache: `$XDG_CACHE_HOME/nyne/overlay/`.
pub fn default_persist_root(cache_dir: &Path) -> PathBuf { cache_dir.join("nyne/overlay") }

/// Base directory for clone lowerdirs: `<cache_dir>/nyne/lower-<pid>`.
///
/// Per-path clone directories are derived via
/// `lower_base(cache_dir, pid).join(relative_to_root(path))`.
pub fn lower_base(target_dir: &Path, pid: Pid) -> PathBuf { target_dir.join(format!("nyne/lower-{pid}")) }

/// Base directory for overlay merged views: `/tmp/nyne-merged-<pid>`.
///
/// Per-path merged directories are derived via `merged_base(pid).join(relative_to_root(path))`.
pub fn merged_base(pid: Pid) -> PathBuf { PathBuf::from(format!("/tmp/nyne-merged-{pid}")) }
