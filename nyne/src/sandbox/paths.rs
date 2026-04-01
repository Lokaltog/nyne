//! Well-known path constructors for sandbox operations.
//!
//! SSOT for all path construction in the sandbox module. No other
//! sandbox submodule uses `format!` or inline `join` chains for
//! well-known paths — they call functions here instead.
//!
//! ## Per-process state layout
//!
//! Every nyne daemon and attach-command child gets a subdirectory under
//! `<state_root>/proc/<pid>/fs/` with five fixed layers:
//!
//! ```text
//! <state_root>/proc/<pid>/fs/
//!   ├── root     pivot_root scaffold
//!   ├── merged   overlay merged view
//!   ├── lower    libgit2 clone lowerdir
//!   ├── upper    overlay upperdir
//!   └── work     overlay workdir (kernel scratch)
//! ```
//!
//! The `state_root` is configurable via `SandboxConfig::state_root`
//! (default `/tmp/nyne`). Cleaning up a process is a single
//! `rm -rf <state_root>/proc/<pid>`.
//!
//! Session/socket paths live in `crate::session` — not here.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use rustix::process::Pid;
use tracing::{debug, warn};

/// Path to the current process UID mapping file (`/proc/self/uid_map`).
///
/// Written after `unshare(CLONE_NEWUSER)` to establish the uid mapping
/// between the new user namespace and its parent. Format: `"<inner> <outer> <count>"`.
pub(super) const UID_MAP: &str = "/proc/self/uid_map";
/// Path to the current process GID mapping file (`/proc/self/gid_map`).
///
/// Must be written after denying `setgroups` (kernel requirement).
/// Format: `"<inner> <outer> <count>"`.
pub(super) const GID_MAP: &str = "/proc/self/gid_map";
/// Path to the current process setgroups control file (`/proc/self/setgroups`).
///
/// Writing `"deny"` to this file is a kernel prerequisite for writing
/// `gid_map` in an unprivileged user namespace. This prevents the
/// unprivileged process from calling `setgroups(2)` to drop supplementary
/// groups (a privilege escalation vector).
pub(super) const SETGROUPS: &str = "/proc/self/setgroups";

/// Per-process state subdirectory (parallels `/proc`).
const PROC_SUBDIR: &str = "proc";
/// Sandbox filesystem scaffolding subdirectory (under `proc/<pid>/`).
const FS_SUBDIR: &str = "fs";

/// Name of the `pivot_root` scaffold directory under `fs/`.
const ROOT_DIR: &str = "root";
/// Name of the overlay merged-view directory under `fs/`.
const MERGED_DIR: &str = "merged";
/// Name of the libgit2 clone lowerdir directory under `fs/`.
const LOWER_DIR: &str = "lower";
/// Name of the overlay upperdir directory under `fs/`.
const UPPER_DIR: &str = "upper";
/// Name of the overlay workdir directory under `fs/` (kernel scratch space).
const WORK_DIR: &str = "work";

/// `<state_root>/proc` — parent of every per-PID state directory.
pub(super) fn proc_root(state_root: &Path) -> PathBuf { state_root.join(PROC_SUBDIR) }
/// Per-process state tree under `<state_root>/proc/<pid>`.
///
/// Precomputes the common path prefix so individual layer accessors are
/// single `join` calls. Also owns cleanup of its state tree via [`reap`].
///
/// [`reap`]: ProcState::reap
pub(super) struct ProcState {
    /// `<state_root>/proc/<pid>` — the process-level state root.
    state: PathBuf,
    /// `<state_root>/proc/<pid>/fs` — parent of all overlay-layer subdirs.
    fs: PathBuf,
}

impl ProcState {
    /// Build paths for the given process under `state_root`.
    pub(super) fn new(state_root: &Path, pid: Pid) -> Self {
        let state = proc_root(state_root).join(pid.to_string());
        Self {
            fs: state.join(FS_SUBDIR),
            state,
        }
    }

    /// Temporary root for `pivot_root`: `<state_root>/proc/<pid>/fs/root`.
    ///
    /// A tmpfs is mounted here as the scaffold for the sandbox filesystem.
    /// After `pivot_root`, this becomes `/` and the old root is detached.
    pub(super) fn newroot(&self) -> PathBuf { self.fs.join(ROOT_DIR) }

    /// Base directory for overlay merged views:
    /// `<state_root>/proc/<pid>/fs/merged`.
    ///
    /// Per-path merged directories are derived via
    /// `proc_state.merged().join(relative_to_root(path))`.
    pub(super) fn merged(&self) -> PathBuf { self.fs.join(MERGED_DIR) }

    /// Base directory for clone lowerdirs:
    /// `<state_root>/proc/<pid>/fs/lower`.
    ///
    /// Per-path clone directories are derived via
    /// `proc_state.lower().join(relative_to_root(path))`.
    pub(super) fn lower(&self) -> PathBuf { self.fs.join(LOWER_DIR) }

    /// Base directory for overlay upperdirs:
    /// `<state_root>/proc/<pid>/fs/upper`.
    ///
    /// Per-path upper directories are derived via
    /// `proc_state.upper().join(relative_to_root(path))`.
    pub(super) fn upper(&self) -> PathBuf { self.fs.join(UPPER_DIR) }

    /// Base directory for overlayfs workdirs:
    /// `<state_root>/proc/<pid>/fs/work`.
    ///
    /// Overlayfs requires a workdir on the same filesystem as upperdir for
    /// kernel scratch space. Per-path work directories are derived via
    /// `proc_state.work().join(relative_to_root(path))`.
    pub(super) fn work(&self) -> PathBuf { self.fs.join(WORK_DIR) }

    /// Best-effort recursive removal of the per-process state tree.
    ///
    /// Safe to call after the owning process has been waitpid-reaped — its
    /// mount namespace is gone, so nothing inside the tree is a mount point
    /// and `remove_dir_all` succeeds.
    ///
    /// Before removal, ensures all directories have owner `rwx`. The
    /// overlayfs kernel creates internal workdir directories (`work/`,
    /// `index/`) with mode `0000`. These persist on the underlying tmpfs
    /// after overlayfs is unmounted via namespace destruction. Without
    /// `CAP_DAC_OVERRIDE` (unavailable when the calling process is in a
    /// child user namespace of the tmpfs owner), `remove_dir_all` cannot
    /// open mode-0000 directories. `chmod` only requires file ownership,
    /// so the supervisor can fix permissions before removal.
    ///
    /// `NotFound` is silent; other errors warn but never fail.
    pub(super) fn reap(&self) {
        ensure_owner_access(&self.state);
        match fs::remove_dir_all(&self.state) {
            Ok(()) => debug!(path = %self.state.display(), "removed per-process state tree"),
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => warn!(path = %self.state.display(), error = %e, "failed to remove per-process state tree"),
        }
    }
}
/// Recursively ensure all directories under `path` have owner `rwx`.
///
/// The overlayfs kernel creates internal workdir directories with mode
/// `0000`. After unmount (via namespace destruction), these persist on
/// the underlying filesystem. `chmod` only requires file ownership —
/// not read/execute permission on the target — so the caller can fix
/// permissions even on mode-0000 directories it owns.
fn ensure_owner_access(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if !meta.is_dir() {
        return;
    }
    if meta.permissions().mode() & 0o700 != 0o700 {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        ensure_owner_access(&entry.path());
    }
}
