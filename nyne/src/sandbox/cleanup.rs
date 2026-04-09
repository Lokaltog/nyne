//! Reap per-process state directories left behind by terminated daemons
//! and attach-command children.
//!
//! Every nyne process that mounts FUSE or enters a sandbox creates a
//! directory tree under `<state_root>/proc/<pid>/` containing its overlay
//! scaffolding (`fs/{root,merged,lower,upper}`). The mounts inside those
//! directories live in the process's private mount namespace and are torn
//! down by the kernel when the namespace dies; the containing directories
//! themselves live in the host mount namespace and persist until removed.
//!
//! Per-PID cleanup is handled by [`super::paths::ProcState::reap`] —
//! this module provides [`reap_stale`] which scans `<state_root>/proc/`
//! and reaps any subtree whose PID is no longer alive (called on
//! `nyne mount` startup to clean up after crashed predecessors).

use std::fs;
use std::path::Path;

use rustix::process::Pid;

use super::paths::{self, ProcState};
use crate::process::is_pid_alive;

/// Remove per-process state trees belonging to dead processes.
///
/// Scans `<state_root>/proc/` for subdirectories named with a PID, and
/// removes those whose PID is no longer alive. Called on `nyne mount`
/// startup so each fresh invocation also cleans up anything a crashed
/// predecessor left behind.
pub(super) fn reap_stale(state_root: &Path) {
    let Ok(entries) = fs::read_dir(paths::proc_root(state_root)) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<i32>().ok())
            .and_then(Pid::from_raw)
        else {
            continue;
        };
        if !is_pid_alive(pid.as_raw_nonzero().get()) {
            ProcState::new(state_root, pid).reap();
        }
    }
}
