use std::path::Path;

use super::*;

/// Tests that `find_cgroup2_mount` locates the cgroup2 mount point.
#[test]
fn find_cgroup2_mount_finds_mount_point() {
    // On any modern Linux system with cgroups v2, this should succeed.
    let mount = find_cgroup2_mount();
    // Don't assert Some — CI environments may not have cgroup2.
    if let Some(path) = mount {
        assert!(path.is_dir(), "cgroup2 mount point should be a directory");
    }
}

/// Tests that `read_pid_cgroup_raw` returns the current process cgroup path.
#[test]
fn read_pid_cgroup_raw_returns_self_cgroup() {
    let path = read_pid_cgroup_raw(Path::new("/proc/self/cgroup"));
    // On systems with cgroups v2 (even hybrid), this should return a path.
    if let Some(cgroup) = path {
        assert!(cgroup.starts_with('/'), "cgroup path should be absolute: {cgroup}");
    }
}

/// Tests that `read_pid_cgroup_raw` returns None for a nonexistent PID.
#[test]
fn read_pid_cgroup_raw_returns_none_for_nonexistent() {
    assert_eq!(read_pid_cgroup_raw(Path::new("/proc/99999999/cgroup")), None);
}

/// Verifies the session cgroup name format.
#[test]
fn session_name_format() {
    assert_eq!(session_name(12345), "pid-12345");
}

/// Tests that `CgroupTracker::new` gracefully returns None when unavailable.
#[test]
fn tracker_new_graceful_fallback() {
    // CgroupTracker::new() should either succeed or return None — never panic.
    let _tracker = CgroupTracker::new();
}

/// Tests that resolve returns None for an untracked PID.
#[test]
fn tracker_resolve_returns_none_for_untracked() {
    // Even if cgroups work, an untracked PID should resolve to None.
    if let Some(tracker) = CgroupTracker::new() {
        assert_eq!(tracker.resolve(std::process::id()), None);
    }
}

/// Tests that tracking a PID allows it to be resolved.
#[test]
fn tracker_track_and_resolve() {
    let Some(tracker) = CgroupTracker::new() else { return };

    let pid = std::process::id();
    tracker.track(pid, ProcessVisibility::All);

    // Our process should now be in the tracked cgroup.
    assert_eq!(tracker.resolve(pid), Some(ProcessVisibility::All));

    // Cleanup: untrack so Drop doesn't leave stale cgroups.
    // Note: untrack removes from sessions map; cgroup dir persists
    // while our process is alive, cleaned up on Drop.
    tracker.untrack(pid);
}

/// Tests that untracking a PID removes it from resolution.
#[test]
fn tracker_untrack_removes_session() {
    let Some(tracker) = CgroupTracker::new() else { return };

    let pid = std::process::id();
    tracker.track(pid, ProcessVisibility::None);
    tracker.untrack(pid);

    // After untrack, resolve should return None (session removed from map).
    // The cgroup dir may still exist (process alive), but we don't match it.
    assert_eq!(tracker.resolve(pid), None);
}
