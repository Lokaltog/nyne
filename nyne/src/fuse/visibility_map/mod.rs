//! Per-process visibility resolution for FUSE requests.
//!
//! Replaces the binary passthrough/non-passthrough model with a three-level
//! [`ProcessVisibility`] system. Processes can be `All` (force-show hidden
//! nodes), `Default` (normal nyne behavior), or `None` (full passthrough).

use std::collections::HashMap;
use std::fs;

use parking_lot::RwLock;
use tracing::debug;

use crate::types::ProcessVisibility;

mod cgroup;
use cgroup::CgroupTracker;

/// Maximum visible length of `/proc/{pid}/comm` (kernel `TASK_COMM_LEN` minus NUL).
const COMM_MAX_LEN: usize = 15;

/// A PID map entry distinguishing explicit overrides from cached lookups.
///
/// The ancestor walk only inherits [`Explicit`](Self::Explicit) entries —
/// cached results must not propagate to child processes (a child's own
/// identity or session membership takes precedence).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VisibilityEntry {
    /// Set via [`VisibilityMap::set_pid`] — inheritable by children through
    /// the ancestor walk.
    Explicit(ProcessVisibility),
    /// Stored by [`VisibilityMap::cache_resolved`] for fast-path hits on
    /// repeat FUSE requests from the same PID. Not inheritable.
    Cached(ProcessVisibility),
}

/// Methods for extracting the visibility level from an entry.
impl VisibilityEntry {
    /// Extracts the visibility level from the entry.
    const fn visibility(self) -> ProcessVisibility {
        match self {
            Self::Explicit(v) | Self::Cached(v) => v,
        }
    }
}

/// Shared visibility state for the FUSE handler and control server.
///
/// Three layers of resolution:
/// 1. **PID overrides** — explicit per-PID entries from `nyne attach --visibility`
///    or `SetVisibility` control requests. Mutable at runtime. Inheritable by
///    children via ancestor walk.
/// 2. **Cgroup membership** — when cgroups v2 is available, child processes
///    auto-inherit their parent's cgroup and visibility is resolved via a single
///    `/proc/{pid}/cgroup` read. Falls back to ancestor walk when unavailable.
/// 3. **Name-based rules** — process comm names mapped to a visibility level.
///    Static rules are built at mount time from config (`passthrough_processes`
///    → `None`) and plugin contributions. Dynamic rules can be added at runtime
///    via `SetVisibility` control requests with a `name` target.
///
/// Resolution falls back to [`ProcessVisibility::Default`] when no layer
/// matches. All resolution results are cached as [`VisibilityEntry::Cached`]
/// for fast-path hits; only explicit overrides propagate via ancestor walk.
pub struct VisibilityMap {
    /// Per-PID visibility entries — explicit overrides and cached resolutions.
    pid_entries: RwLock<HashMap<u32, VisibilityEntry>>,
    /// Process-name → visibility rules, immutable after construction.
    /// Names are pre-truncated to [`COMM_MAX_LEN`] to match kernel truncation.
    name_rules: HashMap<String, ProcessVisibility>,
    /// Runtime name-based rules added via `SetVisibility` control requests.
    /// Checked alongside `name_rules` in `resolve_by_comm`, with dynamic
    /// rules taking precedence over static ones.
    dynamic_name_rules: RwLock<HashMap<String, ProcessVisibility>>,
    /// Optional cgroups v2 tracker for child process visibility inheritance.
    /// `None` when cgroups v2 is unavailable — ancestor walk provides fallback.
    cgroup_tracker: Option<CgroupTracker>,
}

/// Visibility resolution and PID management.
impl VisibilityMap {
    /// Build a new visibility map from name-based rules.
    ///
    /// Each name is truncated to [`COMM_MAX_LEN`] chars to match the kernel's
    /// `/proc/{pid}/comm` truncation — callers can pass full binary names
    /// (e.g., `"typescript-language-server"`) and matching works transparently.
    pub fn new(name_rules: impl IntoIterator<Item = (String, ProcessVisibility)>) -> Self {
        Self {
            pid_entries: RwLock::new(HashMap::new()),
            name_rules: name_rules
                .into_iter()
                .map(|(name, vis)| (truncate_comm(name), vis))
                .collect(),
            dynamic_name_rules: RwLock::new(HashMap::new()),
            cgroup_tracker: None,
        }
    }

    /// Enable cgroups v2 tracking for child process visibility inheritance.
    ///
    /// Attempts to set up a cgroup hierarchy under the daemon's own cgroup.
    /// If cgroups v2 is unavailable (no mount, no write access, hierarchy
    /// constraints), logs a debug message and proceeds without — the ancestor
    /// walk provides equivalent behavior as fallback.
    pub fn with_cgroup_tracking(mut self) -> Self {
        self.cgroup_tracker = CgroupTracker::new();
        if self.cgroup_tracker.is_none() {
            debug!("cgroups v2 unavailable — using ancestor walk for child visibility");
        }
        self
    }

    /// Resolve the visibility level for a process by PID.
    ///
    /// Resolution order:
    /// 1. Direct PID override (explicit `SetVisibility` entry or cached result).
    /// 2. Cgroup membership — if cgroups v2 tracking is active, check whether
    ///    the process belongs to a tracked session cgroup. Children auto-inherit
    ///    cgroups on `fork()`, so this handles reparented processes correctly.
    /// 3. Name-based rule via `/proc/{pid}/comm` — the process's own
    ///    identity takes priority over inherited ancestry.
    /// 4. Ancestor walk — traverse `/proc/{pid}/status` → `PPid` up the
    ///    process tree. Fallback when cgroups are unavailable or miss.
    /// 5. Fall back to [`ProcessVisibility::Default`].
    ///
    /// Steps 2–5 cache their result in the PID override map so subsequent
    /// FUSE requests from the same PID are a single `HashMap` lookup.
    ///
    /// Returns `Default` (fail-open, also cached) if procfs is unavailable
    /// or the process has exited.
    pub fn resolve(&self, pid: u32) -> ProcessVisibility {
        // Fast path: direct PID lookup (explicit override or cached result).
        if let Some(&entry) = self.pid_entries.read().get(&pid) {
            return entry.visibility();
        }

        // Cgroup-based lookup (preferred when available).
        if let Some(vis) = self.cgroup_tracker.as_ref().and_then(|t| t.resolve(pid)) {
            return self.cache_resolved(pid, vis);
        }

        // Name-based rule via /proc/{pid}/comm — process identity takes
        // priority over ancestor inheritance.
        if (!self.name_rules.is_empty() || !self.dynamic_name_rules.read().is_empty())
            && let Some(vis) = self.resolve_by_comm(pid)
        {
            return self.cache_resolved(pid, vis);
        }

        // Ancestor walk — only inherits Explicit entries, not cached results.
        if let Some(vis) = self.resolve_by_ancestors(pid) {
            return self.cache_resolved(pid, vis);
        }

        // No layer matched — cache Default to skip the full resolution chain
        // on subsequent FUSE requests from the same PID.
        self.cache_resolved(pid, ProcessVisibility::Default)
    }

    /// Set an explicit visibility override for a PID.
    ///
    /// Also tracks the process in a cgroup (if cgroups v2 is active) so that
    /// children forked after this call auto-inherit the visibility.
    pub fn set_pid(&self, pid: u32, visibility: ProcessVisibility) {
        self.pid_entries
            .write()
            .insert(pid, VisibilityEntry::Explicit(visibility));
        if let Some(tracker) = &self.cgroup_tracker {
            tracker.track(pid, visibility);
        }
    }

    /// Remove a PID override and untrack from cgroups if active.
    pub fn remove_pid(&self, pid: u32) {
        self.pid_entries.write().remove(&pid);
        if let Some(tracker) = &self.cgroup_tracker {
            tracker.untrack(pid);
        }
    }

    /// Set a dynamic name-based visibility rule.
    ///
    /// The name is truncated to [`COMM_MAX_LEN`] to match kernel behavior.
    /// Dynamic rules take precedence over static (config-time) rules and
    /// invalidate any cached resolutions that relied on name matching.
    pub fn set_name_rule(&self, name: String, visibility: ProcessVisibility) {
        self.dynamic_name_rules.write().insert(truncate_comm(name), visibility);
    }

    /// Return all explicit PID overrides (not cached resolutions).
    pub fn explicit_pid_entries(&self) -> Vec<(u32, ProcessVisibility)> {
        self.pid_entries
            .read()
            .iter()
            .filter_map(|(&pid, &entry)| match entry {
                VisibilityEntry::Explicit(vis) => Some((pid, vis)),
                VisibilityEntry::Cached(_) => None,
            })
            .collect()
    }

    /// Return all dynamic name-based rules.
    pub fn dynamic_name_rules(&self) -> Vec<(String, ProcessVisibility)> {
        self.dynamic_name_rules
            .read()
            .iter()
            .map(|(name, &vis)| (name.clone(), vis))
            .collect()
    }

    /// Cache a resolved visibility for fast-path hits on repeat requests.
    ///
    /// Cached entries are **not inheritable** — the ancestor walk skips them.
    fn cache_resolved(&self, pid: u32, vis: ProcessVisibility) -> ProcessVisibility {
        self.pid_entries.write().insert(pid, VisibilityEntry::Cached(vis));
        vis
    }

    /// Walk the parent PID chain looking for an ancestor with an explicit
    /// PID override.
    ///
    /// Only [`VisibilityEntry::Explicit`] entries are inheritable — cached
    /// resolution results from other processes must not propagate to children.
    ///
    /// Reads `/proc/{pid}/status` to extract `PPid`. Stops at PID 1 (init)
    /// or after [`MAX_ANCESTOR_DEPTH`] hops to avoid pathological loops.
    fn resolve_by_ancestors(&self, pid: u32) -> Option<ProcessVisibility> {
        let mut current = pid;
        for _ in 0..MAX_ANCESTOR_DEPTH {
            let parent = read_ppid(current)?;
            if parent <= 1 {
                return None;
            }
            if let Some(&VisibilityEntry::Explicit(vis)) = self.pid_entries.read().get(&parent) {
                return Some(vis);
            }
            current = parent;
        }
        None
    }

    /// Read `/proc/{pid}/comm` and check against name-based rules.
    ///
    /// Dynamic rules (set at runtime via control requests) take precedence
    /// over static rules (from config).
    fn resolve_by_comm(&self, pid: u32) -> Option<ProcessVisibility> {
        let comm = fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        let comm = comm.trim_end();
        self.dynamic_name_rules
            .read()
            .get(comm)
            .copied()
            .or_else(|| self.name_rules.get(comm).copied())
    }
}

/// Maximum number of parent hops when walking the ancestor chain.
///
/// Prevents unbounded traversal in pathological cases. 32 is generous —
/// typical shell → command chains are 3–5 deep.
const MAX_ANCESTOR_DEPTH: usize = 32;

/// Read the parent PID from `/proc/{pid}/status`.
///
/// Parses the `PPid:` line. Returns `None` if procfs is unavailable
/// or the process has exited.
fn read_ppid(pid: u32) -> Option<u32> {
    fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()?
        .lines()
        .find(|line| line.starts_with("PPid:"))
        .and_then(|line| line.split_ascii_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
}

/// Truncate a process name to [`COMM_MAX_LEN`] chars, matching the kernel's
/// `/proc/{pid}/comm` truncation behavior.
fn truncate_comm(name: String) -> String {
    if name.len() > COMM_MAX_LEN {
        name[..COMM_MAX_LEN].to_owned()
    } else {
        name
    }
}

#[cfg(test)]
mod tests;
