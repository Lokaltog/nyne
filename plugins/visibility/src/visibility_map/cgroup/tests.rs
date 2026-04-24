use std::path::Path;

use rstest::rstest;

use super::*;

/// Tests that `find_cgroup2_mount` locates the cgroup2 mount point.
#[rstest]
fn find_cgroup2_mount_finds_mount_point() {
    // On any modern Linux system with cgroups v2, this should succeed.
    let mount = find_cgroup2_mount();
    // Don't assert Some — CI environments may not have cgroup2.
    if let Some(path) = mount {
        assert!(path.is_dir(), "cgroup2 mount point should be a directory");
    }
}

/// Tests that `read_pid_cgroup_raw` returns the current process cgroup path.
#[rstest]
fn read_pid_cgroup_raw_returns_self_cgroup() {
    let path = read_pid_cgroup_raw(Path::new("/proc/self/cgroup"));
    // On systems with cgroups v2 (even hybrid), this should return a path.
    if let Some(cgroup) = path {
        assert!(cgroup.starts_with('/'), "cgroup path should be absolute: {cgroup}");
    }
}

/// Tests that `read_pid_cgroup_raw` returns None for a nonexistent PID.
#[rstest]
fn read_pid_cgroup_raw_returns_none_for_nonexistent() {
    assert_eq!(read_pid_cgroup_raw(Path::new("/proc/99999999/cgroup")), None);
}

/// Verifies the session cgroup name format.
#[rstest]
fn session_name_format() {
    assert_eq!(session_name(12345), "pid-12345");
}

/// Tests CgroupTracker session state across `track`/`untrack` operations.
/// Each case applies a sequence of ops and then asserts `resolve` for the
/// current PID. Skipped cleanly on systems without cgroup v2 support.
#[rstest]
#[case::untracked(&[], None)]
#[case::track_makes_resolvable(
    &[Op::Track(ProcessVisibility::All)],
    Some(ProcessVisibility::All),
)]
#[case::untrack_removes_session(
    &[Op::Track(ProcessVisibility::None), Op::Untrack],
    None,
)]
fn tracker_session_operations(#[case] ops: &[Op], #[case] expected: Option<ProcessVisibility>) {
    let Some(tracker) = CgroupTracker::new() else { return };
    let pid = std::process::id();
    for op in ops {
        match op {
            Op::Track(v) => tracker.track(pid, *v),
            Op::Untrack => tracker.untrack(pid),
        }
    }
    assert_eq!(tracker.resolve(pid), expected);
    // Clean up so Drop doesn't leave stale cgroups and so successive cases start fresh.
    tracker.untrack(pid);
}

enum Op {
    Track(ProcessVisibility),
    Untrack,
}
