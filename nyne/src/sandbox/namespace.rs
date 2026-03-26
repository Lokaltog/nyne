//! Linux namespace creation and entry (user, mount, PID, UTS).
//!
//! Provides safe wrappers around `unshare(2)` and `setns(2)` for the
//! four namespace types used by the sandbox:
//!
//! - **User namespace** — grants `CAP_SYS_ADMIN` for unprivileged mount
//!   operations, then remaps back to the real uid/gid after mounts complete.
//! - **Mount namespace** — isolates the sandbox's mount table from the host.
//! - **PID namespace** — gives the command its own PID 1 (init) for clean
//!   signal forwarding and zombie reaping.
//! - **UTS namespace** — sets a distinct hostname for shell prompt display.
//!
//! All `unshare` calls go through a single safe wrapper that asserts
//! `UnshareFlags::FILES` is never used (the only flag that would create
//! cross-thread UB).

use std::fs as stdfs;
use std::os::fd::{AsFd, OwnedFd};

use color_eyre::eyre::{Result, WrapErr};
use rustix::fs::{self, Mode, OFlags};
use rustix::process::{Pid, getgid, getuid};
use rustix::thread::{LinkNameSpaceType, UnshareFlags};
use rustix::{system, thread};
use tracing::{debug, trace};

use super::paths;
use crate::err::ErrnoExt;

/// Safe wrapper around `unshare_unsafe` for flags that don't affect fd tables.
///
/// The only flag that makes `unshare` genuinely unsafe is `FILES` (it can
/// hide fds between threads). This wrapper asserts `FILES` is not set and
/// encapsulates the single `unsafe` block for the entire module.
#[allow(unsafe_code)]
fn unshare(flags: UnshareFlags, label: &str) -> Result<()> {
    assert!(
        !flags.contains(UnshareFlags::FILES),
        "UnshareFlags::FILES must not be used — it breaks cross-thread fd visibility"
    );
    // SAFETY: Without FILES, unshare only affects namespace membership of the
    // calling thread/process — no fd table manipulation, no cross-thread UB.
    syscall_try!(unsafe { thread::unshare_unsafe(flags) }, "{label}");
    Ok(())
}
/// Handles to a process's user and mount namespace fds.
///
/// Opened from `/proc/<pid>/ns/{user,mnt}`. Used by the command child
/// to enter the daemon's namespace before exec.
pub(super) struct Namespace {
    user_ns: OwnedFd,
    mnt_ns: OwnedFd,
}

impl Namespace {
    /// Open namespace file descriptors for the given pid.
    ///
    /// Opens `/proc/<pid>/ns/user` and `/proc/<pid>/ns/mnt` as read-only fds.
    /// These fds are passed to [`enter`](Self::enter) to join the target
    /// process's user and mount namespaces via `setns(2)`.
    pub(super) fn open_from_pid(pid: Pid) -> Result<Self> {
        debug!(pid = pid.as_raw_pid(), "opening namespace fds");

        let user_path = paths::ns_user(pid);
        let user_ns = syscall_try!(
            fs::open(&user_path, OFlags::RDONLY, Mode::empty()),
            "opening {}",
            user_path.display()
        );

        let mnt_path = paths::ns_mnt(pid);
        let mnt_ns = syscall_try!(
            fs::open(&mnt_path, OFlags::RDONLY, Mode::empty()),
            "opening {}",
            mnt_path.display()
        );

        trace!("namespace fds acquired");

        Ok(Self { user_ns, mnt_ns })
    }

    /// Enter this namespace (user first, then mount).
    ///
    /// Order matters: the user namespace must be entered before the mount
    /// namespace because `setns(CLONE_NEWNS)` requires `CAP_SYS_ADMIN` in
    /// the target mount namespace's owning user namespace. Entering the user
    /// namespace first grants the necessary capability.
    ///
    /// Consumes `self` — the namespace fds are closed after entry.
    pub(super) fn enter(self) -> Result<()> {
        debug!("entering user namespace");
        syscall_try!(
            thread::move_into_link_name_space(self.user_ns.as_fd(), Some(LinkNameSpaceType::User)),
            "setns(CLONE_NEWUSER)"
        );

        debug!("entering mount namespace");
        syscall_try!(
            thread::move_into_link_name_space(self.mnt_ns.as_fd(), Some(LinkNameSpaceType::Mount)),
            "setns(CLONE_NEWNS)"
        );

        Ok(())
    }
}

/// Write uid/gid maps for the current process's user namespace.
///
/// Must deny `setgroups` before writing `gid_map` (kernel requirement).
/// Map format: `"<inside_ns_id> <outside_ns_id> <count>"`.
fn write_id_maps(uid_map: &str, gid_map: &str) -> Result<()> {
    stdfs::write(paths::UID_MAP, uid_map).wrap_err("writing uid_map")?;
    trace!(map = %uid_map, "uid_map written");

    // Must deny setgroups before writing gid_map (kernel requirement).
    stdfs::write(paths::SETGROUPS, "deny").wrap_err("writing setgroups")?;

    stdfs::write(paths::GID_MAP, gid_map).wrap_err("writing gid_map")?;
    trace!(map = %gid_map, "gid_map written");

    Ok(())
}

/// Create a new user+mount namespace via `unshare` and write uid/gid maps.
///
/// Captures uid/gid before `unshare` (afterwards they become 65534/overflow
/// until maps are written). Maps the current user to root inside the
/// namespace, enabling unprivileged `mount()` syscalls.
pub(super) fn unshare_user_mount() -> Result<()> {
    let uid = getuid();
    let gid = getgid();

    debug!(uid = uid.as_raw(), gid = gid.as_raw(), "creating user+mount namespace");

    unshare(
        UnshareFlags::NEWUSER | UnshareFlags::NEWNS,
        "unshare(CLONE_NEWUSER | CLONE_NEWNS)",
    )?;

    // Map host uid/gid → root (0) inside the namespace for mount privileges.
    write_id_maps(&format!("0 {} 1", uid.as_raw()), &format!("0 {} 1", gid.as_raw()))?;

    debug!("namespace created, uid/gid maps written");
    Ok(())
}

/// Create a private mount namespace for overlay setup.
///
/// Only unshares `CLONE_NEWNS` — does NOT create a nested user
/// namespace. This avoids mount locking: inherited mounts remain
/// unlocked and can be unmounted or mounted over.
///
/// The caller must already have `CAP_SYS_ADMIN` in the current user
/// namespace (i.e., after `setns` into the daemon's user namespace).
pub(super) fn unshare_private_mount() -> Result<()> {
    debug!("creating private mount namespace");
    unshare(UnshareFlags::NEWNS, "unshare(CLONE_NEWNS)")?;
    super::mnt::private()?;
    debug!("private mount namespace created");
    Ok(())
}

/// Drop root identity by creating a nested user namespace.
///
/// The daemon's user namespace maps host uid/gid → 0 (root) so it can
/// perform mount operations. After all mounts are set up, the command
/// child calls this to appear as the real user instead of root.
///
/// Creates a new user namespace with uid/gid maps:
///   `{real_uid} 0 1` — maps `real_uid` in new ns → uid 0 in parent ns → host uid
///   `{real_gid} 0 1` — same for gid
pub(super) fn unshare_user_remap(uid: u32, gid: u32) -> Result<()> {
    debug!(uid, gid, "creating user namespace to remap uid/gid");

    unshare(UnshareFlags::NEWUSER, "unshare(CLONE_NEWUSER)")?;

    // Map real uid/gid in new ns → uid/gid 0 in parent ns (which itself
    // maps to the real host uid/gid).
    write_id_maps(&format!("{uid} 0 1"), &format!("{gid} 0 1"))?;

    debug!("user namespace created, now running as uid={uid} gid={gid}");
    Ok(())
}

/// Create a UTS namespace and set the hostname.
///
/// Isolates the hostname from the host so the shell prompt can display
/// a distinct identity (e.g., `user@nyne-sandbox`). Cheap operation —
/// just copies two strings (hostname + domainname).
pub(super) fn unshare_uts(hostname: &str) -> Result<()> {
    debug!(hostname, "creating UTS namespace");

    unshare(UnshareFlags::NEWUTS, "unshare(CLONE_NEWUTS)")?;

    syscall_try!(system::sethostname(hostname.as_bytes()), "sethostname({hostname})");

    debug!(hostname, "UTS namespace created, hostname set");
    Ok(())
}

/// Create a PID namespace via `unshare`.
///
/// The calling process does NOT enter the new PID namespace — its next
/// `fork()` child becomes PID 1 in the new namespace. That child must
/// remount `/proc` to reflect the new PID namespace before dropping
/// mount capabilities (i.e., before `unshare_user_remap`).
pub(super) fn unshare_pid() -> Result<()> {
    debug!("creating PID namespace");

    unshare(UnshareFlags::NEWPID, "unshare(CLONE_NEWPID)")?;

    debug!("PID namespace created (next fork will be PID 1)");
    Ok(())
}
