//! Mount primitives for sandbox construction.
//!
//! SSOT for all mount syscalls in the sandbox module. Every `rustix::mount`
//! call goes through a function here — no other sandbox submodule calls mount
//! directly.
//!
//! Functions are designed to be called with `mnt::` prefix, so names are short
//! and read as verbs: `mnt::overlay(...)`, `mnt::bind(...)`, `mnt::pivot(...)`.

use std::env;
use std::ffi::CString;
use std::os::fd::OwnedFd;
use std::path::Path;

use color_eyre::eyre::{Result, WrapErr};
use rustix::fs::CWD;
use rustix::mount::{
    MountFlags, MountPropagationFlags, UnmountFlags, mount, mount_bind_recursive, mount_change, mount_remount, unmount,
};
use rustix::process;
use tracing::{debug, trace};

use crate::err::ErrnoExt;

/// Mount a tmpfs at `target` with the given size limit.
#[allow(clippy::expect_used)] // paths cannot contain NUL bytes
/// Mount a tmpfs at the given path with the specified size limit.
///
/// The `size` parameter is passed directly as the `size=` mount option
/// (e.g., `"64m"`). The target directory must already exist. Used for
/// the newroot scaffold and the isolated `/tmp` inside the sandbox.
pub fn tmpfs(target: &Path, size: &str) -> Result<()> {
    let data = CString::new(format!("size={size}")).expect("mount data contains no NUL");
    syscall_try!(
        mount("tmpfs", target, "tmpfs", MountFlags::empty(), &*data),
        "mounting tmpfs at {}",
        target.display()
    );
    trace!(target = %target.display(), size, "tmpfs mounted");
    Ok(())
}

/// Mount a proc filesystem at the given path.
///
/// Mounts a new `procfs` instance. In the sandbox, `/proc` is initially
/// bind-mounted from the host (for `uid_map` writes), then remounted by
/// the PID namespace init process to reflect the sandbox's PID namespace.
pub fn proc(target: &Path) -> Result<()> {
    syscall_try!(
        mount("proc", target, "proc", MountFlags::empty(), None),
        "mounting proc at {}",
        target.display()
    );
    trace!(target = %target.display(), "proc mounted");
    Ok(())
}

/// Mount an overlayfs at `target` with the given layer paths.
#[allow(clippy::expect_used)] // paths cannot contain NUL bytes
/// Mount an overlayfs at `target` with the given lower, upper, and work directories.
///
/// Overlayfs presents a unified view: reads fall through to `lower` (the
/// immutable clone), writes are captured in `upper` (persistent across
/// sessions), and `work` is used internally by the kernel for atomic
/// copy-up operations. All four directories must already exist.
pub fn overlay(target: &Path, lower: &Path, upper: &Path, work: &Path) -> Result<()> {
    let opts = format!(
        "lowerdir={},upperdir={},workdir={}",
        lower.display(),
        upper.display(),
        work.display(),
    );
    debug!(target = %target.display(), opts = %opts, "mounting overlayfs");
    let opts_c = CString::new(opts.as_str()).expect("mount data contains no NUL");
    syscall_try!(
        mount("overlay", target, "overlay", MountFlags::empty(), &*opts_c),
        "mounting overlayfs at {} (lower={}, upper={}, work={})",
        target.display(),
        lower.display(),
        upper.display(),
        work.display()
    );
    debug!(target = %target.display(), "overlayfs mounted");
    Ok(())
}

/// Recursive bind-mount `source` onto `target`.
///
/// Uses `MS_BIND | MS_REC` to clone the entire mount tree. To apply
/// additional flags (e.g. `MS_RDONLY`), call [`remount`] separately —
/// the initial bind ignores most flags per kernel semantics.
pub fn bind(source: &Path, target: &Path) -> Result<()> {
    syscall_try!(
        mount_bind_recursive(source, target),
        "bind-mounting {} to {}",
        source.display(),
        target.display()
    );

    debug!(
        source = %source.display(),
        target = %target.display(),
        "bind-mounted",
    );
    Ok(())
}

/// Mark the entire mount tree as private (no propagation).
///
/// Prevents subsequent mounts from leaking into parent namespaces.
pub fn private() -> Result<()> {
    syscall_try!(
        mount_change("/", MountPropagationFlags::REC | MountPropagationFlags::PRIVATE),
        "marking root mount tree private"
    );
    trace!("mount tree marked private");
    Ok(())
}

/// Remount a bind mount with the given flags.
///
/// Only affects the mount at `target` — submounts retain their own flags.
/// Common usage: `remount(path, MountFlags::RDONLY)` for read-only.
pub fn remount(target: &Path, flags: MountFlags) -> Result<()> {
    // Must include BIND for bind-mount remounts — without it the kernel
    // treats this as a regular remount which requires CAP_SYS_ADMIN on
    // the mount's original filesystem, not just the bind mount.
    syscall_try!(
        mount_remount(target, MountFlags::BIND | flags, ""),
        "remounting {} with flags {:?}",
        target.display(),
        flags
    );
    trace!(target = %target.display(), flags = ?flags, "remounted");
    Ok(())
}

/// Lazily detach a mount point.
///
/// The mount becomes invisible to new path lookups immediately, but
/// remains alive for existing fd references until they are closed.
pub fn detach(target: &Path) -> Result<()> {
    syscall_try!(
        unmount(target, UnmountFlags::DETACH),
        "detaching mount at {}",
        target.display()
    );
    trace!(target = %target.display(), "mount detached");
    Ok(())
}

/// Clone a mount into a detached (anonymous) mount tree.
///
/// Uses `open_tree(OPEN_TREE_CLONE)` — the cloned mount is not attached
/// to any namespace's mount tree. Use [`attach`] to place it.
pub fn clone_mount(path: &Path) -> Result<OwnedFd> {
    use rustix::mount::{OpenTreeFlags, open_tree};

    let fd = syscall_try!(
        open_tree(
            CWD,
            path,
            OpenTreeFlags::OPEN_TREE_CLONE | OpenTreeFlags::OPEN_TREE_CLOEXEC,
        ),
        "cloning mount at {}",
        path.display()
    );
    debug!(path = %path.display(), "mount cloned");
    Ok(fd)
}

/// Attach a detached mount (from [`clone_mount`]) at `target`.
///
/// Uses `move_mount(MOVE_MOUNT_F_EMPTY_PATH)` to place the anonymous
/// mount tree at the target path.
pub fn attach(source_fd: &OwnedFd, target: &Path) -> Result<()> {
    use rustix::mount::{MoveMountFlags, move_mount};

    syscall_try!(
        move_mount(source_fd, "", CWD, target, MoveMountFlags::MOVE_MOUNT_F_EMPTY_PATH,),
        "attaching mount at {}",
        target.display()
    );
    debug!(target = %target.display(), "mount attached");
    Ok(())
}

/// Pivot root using the `pivot_root(".", ".")` trick.
///
/// Changes directory into `new_root`, pivots, then lazily detaches the
/// old root (which is stacked on top of the new mount point). No
/// `put_old` directory needed — the old root is unmounted in place.
pub fn pivot(new_root: &Path) -> Result<()> {
    env::set_current_dir(new_root).wrap_err_with(|| format!("chdir to {}", new_root.display()))?;

    syscall_try!(process::pivot_root(".", "."), "pivot_root(\".\", \".\")");

    // Old root is now stacked on top of the new root mount point.
    // Lazily detach it — invisible to new path lookups immediately.
    syscall_try!(unmount(".", UnmountFlags::DETACH), "detaching old root after pivot");

    env::set_current_dir("/").wrap_err("chdir to /")?;
    debug!("pivoted into new root (old root detached)");

    Ok(())
}
