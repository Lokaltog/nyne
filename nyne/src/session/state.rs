use rustix::process::{Pid, test_kill_process};

/// Check whether a process is alive (equivalent to `kill(pid, 0)`).
pub fn is_pid_alive(pid: i32) -> bool {
    let Some(pid) = Pid::from_raw(pid) else {
        return false;
    };
    test_kill_process(pid).is_ok()
}
