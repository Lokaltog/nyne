//! cgroups v2 process tracking for visibility inheritance.
//!
//! When available, cgroups provide reliable child process tracking:
//! children auto-inherit their parent's cgroup on `fork()`, so visibility
//! resolution is a single `/proc/{pid}/cgroup` read + prefix match — no
//! ancestor walk needed.
//!
//! Falls back gracefully to the ancestor walk in [`super::VisibilityMap`]
//! when cgroups v2 is unavailable (no cgroup2 mount, no write access, or
//! cgroup hierarchy constraints prevent child cgroup creation).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use parking_lot::RwLock;
use tracing::{debug, warn};

use crate::types::ProcessVisibility;

/// Tracks attached processes via cgroups v2 for visibility inheritance.
///
/// Each `nyne attach --visibility=X` session gets its own cgroup under
/// a shared `nyne/` base directory. Child processes auto-inherit the
/// cgroup on `fork()`, so visibility resolution is a single
/// `/proc/{pid}/cgroup` read + prefix match.
pub(super) struct CgroupTracker {
    /// Absolute filesystem path to the nyne cgroup base directory.
    /// e.g., `/sys/fs/cgroup/user.slice/.../nyne/`
    base_dir: PathBuf,
    /// Cgroup path prefix for matching `/proc/{pid}/cgroup` entries.
    /// e.g., `/user.slice/.../nyne/`
    cgroup_prefix: String,
    /// Session name → visibility mapping.
    sessions: RwLock<HashMap<String, ProcessVisibility>>,
}

/// Cgroup-based process tracking for visibility inheritance.
impl CgroupTracker {
    /// Attempt to set up cgroups v2 tracking.
    ///
    /// Returns `None` if cgroups v2 is unavailable or we lack write access.
    /// The caller should proceed without cgroup tracking — the ancestor walk
    /// in [`super::VisibilityMap`] provides equivalent (if slower) behavior.
    pub(super) fn new() -> Option<Self> {
        let self_cgroup = read_pid_cgroup_raw(Path::new("/proc/self/cgroup"))?;

        let base_dir = find_cgroup2_mount()?
            .join(self_cgroup.trim_start_matches('/'))
            .join("nyne");

        let mut cgroup_prefix = self_cgroup.trim_end_matches('/').to_owned();
        cgroup_prefix.push_str("/nyne/");

        // Create the base directory. If this fails (permissions, read-only mount,
        // cgroup subtree_control constraints), we gracefully fall back.
        if let Err(e) = fs::create_dir_all(&base_dir) {
            debug!(
                path = %base_dir.display(),
                error = %e,
                "cannot create cgroup base dir — falling back to ancestor walk"
            );
            return None;
        }

        // Clean up stale session cgroups from previous daemon runs.
        cleanup_empty_children(&base_dir);

        debug!(
            base = %base_dir.display(),
            prefix = %cgroup_prefix,
            "cgroups v2 tracking initialized"
        );

        Some(Self {
            base_dir,
            cgroup_prefix,
            sessions: RwLock::new(HashMap::new()),
        })
    }

    /// Track a process in a dedicated cgroup for visibility inheritance.
    ///
    /// Creates a session cgroup and moves the process into it. Children
    /// forked after this call auto-inherit the cgroup.
    pub(super) fn track(&self, pid: u32, visibility: ProcessVisibility) {
        let name = session_name(pid);
        let dir = self.base_dir.join(&name);

        if let Err(e) = fs::create_dir_all(&dir) {
            warn!(pid, path = %dir.display(), error = %e, "failed to create session cgroup");
            return;
        }

        // Move the process into the cgroup by writing its PID to cgroup.procs.
        if let Err(e) = fs::write(dir.join("cgroup.procs"), pid.to_string()) {
            warn!(pid, error = %e, "failed to move process into cgroup");
            // Clean up the empty cgroup dir on failure.
            let _ = fs::remove_dir(&dir);
            return;
        }

        self.sessions.write().insert(name, visibility);
        debug!(pid, %visibility, "process tracked in cgroup");
    }

    /// Resolve visibility for a PID via cgroup membership.
    ///
    /// Reads `/proc/{pid}/cgroup` and checks if the process belongs to
    /// one of our tracked session cgroups.
    pub(super) fn resolve(&self, pid: u32) -> Option<ProcessVisibility> {
        let cgroup_path = read_pid_cgroup_raw(Path::new(&format!("/proc/{pid}/cgroup")))?;
        // The process may be in a sub-cgroup created by the tracked process —
        // take only the first path component after our prefix to match the session name.
        self.sessions
            .read()
            .get(cgroup_path.strip_prefix(&self.cgroup_prefix)?.split('/').next()?)
            .copied()
    }

    /// Stop tracking a session and attempt to clean up its cgroup.
    ///
    /// The cgroup directory is removed only if empty (no processes remain).
    /// If children are still running, the directory persists — their FUSE
    /// requests continue to be resolved via [`resolve`].
    pub(super) fn untrack(&self, pid: u32) {
        let name = session_name(pid);
        self.sessions.write().remove(&name);
        // Best-effort removal — rmdir fails on non-empty cgroups.
        let _ = fs::remove_dir(self.base_dir.join(&name));
    }
}

/// Cleans up all session cgroups and the base directory on drop.
impl Drop for CgroupTracker {
    /// Removes all session cgroups and the base directory.
    fn drop(&mut self) {
        // Best-effort cleanup of all session cgroups and the base dir.
        let sessions = self.sessions.read();
        for name in sessions.keys() {
            let _ = fs::remove_dir(self.base_dir.join(name));
        }
        drop(sessions);
        let _ = fs::remove_dir(&self.base_dir);
    }
}

/// Session cgroup name for a tracked PID.
fn session_name(pid: u32) -> String { format!("pid-{pid}") }

/// Find the cgroup2 mount point by parsing `/proc/self/mountinfo`.
///
/// mountinfo format per line:
/// `id parent major:minor root mount-point options - fstype source super-options`
///
/// We look for the `cgroup2` fstype after the `-` separator.
fn find_cgroup2_mount() -> Option<PathBuf> {
    for line in fs::read_to_string("/proc/self/mountinfo").ok()?.lines() {
        let (before_sep, after_sep) = line.split_once(" - ")?;
        if after_sep.split_ascii_whitespace().next()? == "cgroup2" {
            return before_sep.split_ascii_whitespace().nth(4).map(PathBuf::from);
        }
    }
    None
}

/// Parse a cgroup file, extracting the cgroups v2 path.
///
/// cgroups v2 uses the unified hierarchy identified by `0::` prefix.
/// Hybrid systems have additional cgroups v1 lines — we skip those.
fn read_pid_cgroup_raw(path: &Path) -> Option<String> {
    for line in fs::read_to_string(path).ok()?.lines() {
        if let Some(cgroup_path) = line.strip_prefix("0::") {
            return Some(cgroup_path.to_owned());
        }
    }
    None
}

/// Remove empty child cgroup directories (stale sessions from previous runs).
fn cleanup_empty_children(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|ft| ft.is_dir()) {
            // rmdir only succeeds on empty cgroups (no processes).
            let _ = fs::remove_dir(entry.path());
        }
    }
}

#[cfg(test)]
mod tests;
