//! Project storage and sandbox isolation for FUSE mounts.
//!
//! Two concerns, split between daemon and command child:
//!
//! ## Daemon-side: project storage
//!
//! [`prepare_project_storage`] prepares the backing path for FUSE daemon I/O.
//! In passthrough mode, the project is bind-mounted directly. In overlay
//! modes (snapshot/hardlink), a clone-backed overlayfs is mounted at a
//! separate merged path with persistent upperdirs in `~/.cache/nyne/overlay/`.
//!
//! ## Command-child-side: sandbox isolation
//!
//! [`setup`] builds an isolated root filesystem via `pivot_root`:
//! - Host root entries are bind-mounted selectively (RO by default)
//! - `/dev` and `/run` are bind-mounted RW (PTYs, runtime sockets)
//! - `/home/<user>/` exposes only XDG dirs (RW bind mounts)
//! - FUSE is mounted at `/code` — the single project entry point
//! - `/proc` is bind-mounted from host (needed for `uid_map` writes during
//!   user remap), then remounted by the PID namespace init process
//! - `/tmp` gets an isolated tmpfs
//!
//! Mount stacking order (top to bottom, command child view):
//!
//! ```text
//! Passthrough:          Overlay (snapshot/hardlink):
//! /code → FUSE          /code → FUSE
//!         real fs                overlayfs (clone-backed)
//!                                real fs (via clone lowerdir)
//! ```

use std::fs::{self, File};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{ContextCompat, Result, WrapErr};
use directories::BaseDirs;
use rustix::mount::MountFlags;
use rustix::process;
use tracing::{debug, info};

use super::{PROJECT_CLONERS, mnt, namespace, paths};
use crate::config::{BindMount, StorageStrategy};
use crate::path_utils::PathExt;

/// Size limit for structural tmpfs mounts (newroot, /tmp).
///
/// These tmpfs instances only hold directory stubs for mount points and
/// the isolated `/tmp`. The actual project data lives on the FUSE mount
/// or overlay, not on these tmpfs instances.
const TMPFS_SIZE: &str = "2G";

/// Build the isolated sandbox root filesystem and mount FUSE at `mount_root`.
///
/// Called by the command child after entering the daemon's namespace. The
/// sequence is order-sensitive:
///
/// 1. Clone the FUSE mount into a detached mount tree (survives namespace transitions)
/// 2. Create a private mount namespace (no nested user ns, avoids mount locking)
/// 3. Detach FUSE from the project dir so bind mounts copy host fs, not FUSE
/// 4. Build newroot (tmpfs scaffold, host bind mounts, XDG dirs)
/// 5. Apply user-configured bind mounts (while host paths are still visible)
/// 6. `pivot_root` into newroot
/// 7. Attach the FUSE clone at `mount_root`
/// 8. Remount root tmpfs read-only (layered mounts keep their own flags)
pub(super) fn setup(fuse_path: &Path, bind_mounts: &[BindMount], state_root: &Path, mount_root: &Path) -> Result<()> {
    let base_dirs = BaseDirs::new();

    // Clone the FUSE mount into a detached mount tree while still in the
    // daemon's namespace. open_tree(OPEN_TREE_CLONE) creates an anonymous
    // clone that can be attached in any namespace via move_mount —
    // unlike plain fds, these survive cross-namespace transitions.
    let fuse_fd = mnt::clone_mount(fuse_path).wrap_err("cloning FUSE mount")?;
    debug!(path = %fuse_path.display(), "FUSE mount cloned");

    // Fork a private mount namespace so our mounts don't affect
    // the daemon's namespace. Only CLONE_NEWNS — no nested user
    // namespace, so inherited mounts are NOT locked.
    namespace::unshare_private_mount()?;

    // Detach FUSE from the project dir so subsequent bind mounts copy
    // the host filesystem (not the FUSE layer). The cloned mount tree
    // is independent and unaffected by this detach.
    mnt::detach(fuse_path)?;
    debug!(path = %fuse_path.display(), "FUSE detached");

    let newroot = paths::ProcState::new(state_root, process::getpid()).newroot();

    build_newroot(&newroot, base_dirs.as_ref(), mount_root)?;

    // Apply user-configured bind mounts before pivot — sources are
    // still visible on the host filesystem at this point.
    apply_bind_mounts(&newroot, bind_mounts)?;

    mnt::pivot(&newroot)?;

    // Attach the FUSE clone at mount_root — the single entry point for
    // all project access inside the sandbox.
    dirs(&[mount_root])?;
    mnt::attach(&fuse_fd, mount_root)?;
    info!(path = %mount_root.display(), "FUSE mounted at sandbox mount root");

    // Lock down the root tmpfs. Mounts layered on top (FUSE, XDG
    // bind mounts, /dev, /run, /proc, /tmp) retain their own flags.
    mnt::remount(Path::new("/"), MountFlags::RDONLY)?;
    debug!("root remounted read-only");

    debug!("sandbox setup complete");

    Ok(())
}
/// Prepare the project backing path for the FUSE daemon.
///
/// The returned path is what the daemon uses for all filesystem I/O. All
/// per-process scaffolding lives under
/// `<state_root>/proc/<pid>/fs/{merged,lower,upper}`.
///
/// - **Passthrough**: bind-mounts the project directory RW at the merged
///   path. No clone, no overlayfs — writes hit the real filesystem.
/// - **Snapshot / Hardlink**: clones the project into the `lower/` layer,
///   then mounts overlayfs at `merged/` with `upper/` as the write sink.
pub fn prepare_project_storage(path: &Path, state_root: &Path, strategy: StorageStrategy) -> Result<PathBuf> {
    let proc = paths::ProcState::new(state_root, process::getpid());
    let rel = path.relative_to(Path::new("/"));

    // Both strategies use the same merged path as the daemon's working directory.
    let merged = proc.merged().join(&rel);
    fs::create_dir_all(&merged).wrap_err_with(|| format!("creating merged dir {}", merged.display()))?;

    match strategy {
        StorageStrategy::Passthrough => {
            mnt::bind(path, &merged)?;
            info!(
                path = %path.display(),
                merged = %merged.display(),
                "project bind-mounted (passthrough)"
            );
        }
        strategy @ (StorageStrategy::Snapshot | StorageStrategy::Hardlink) => {
            // Clone project into a stable lower dir for use as overlayfs lowerdir.
            let lower = proc.lower().join(&rel);
            PROJECT_CLONERS
                .first()
                .map(|f| f())
                .wrap_err("no project cloner registered — is the git plugin linked?")?
                .clone_project(path, &lower, strategy)?;

            // Make the clone read-only: bind onto self to create a mount point,
            // then remount RO. Defense-in-depth — overlayfs never writes to
            // lowerdir, but this prevents accidental mutation.
            mnt::bind(&lower, &lower)?;
            mnt::remount(&lower, MountFlags::RDONLY)?;
            debug!(lower = %lower.display(), "clone lowerdir remounted read-only");

            let upper = proc.upper().join(&rel);
            mount_overlay_layers(&merged, &lower, &upper, &proc.work().join(&rel))?;
            info!(
                path = %path.display(),
                lower = %lower.display(),
                merged = %merged.display(),
                upper = %upper.display(),
                "project overlay mounted (clone-backed)"
            );
        }
    }

    Ok(merged)
}
/// Populate the new root with bind-mounted host entries, isolated `/tmp`, and XDG dirs.
///
/// Creates a tmpfs at `newroot` as the scaffold, then layers on:
/// 1. Selective host `/` bind mounts (skipping entries handled elsewhere)
/// 2. A fresh tmpfs `/tmp` (prevents temp file leakage to/from host)
/// 3. XDG directories (config, cache, data, state) as RW bind mounts
///
/// If `base_dirs` is `None` (XDG dirs unavailable), the home directory
/// step is skipped entirely — no home is visible inside the sandbox.
fn build_newroot(newroot: &Path, base_dirs: Option<&BaseDirs>, mount_root: &Path) -> Result<()> {
    dirs(&[newroot])?;
    mnt::tmpfs(newroot, TMPFS_SIZE)?;

    // Enumerate host root entries and selectively bind-mount them,
    // giving precise control over what's visible in the sandbox.
    // /proc is included — needed for /proc/self/uid_map writes during
    // user remap. The PID namespace init process remounts /proc after
    // fork to scope it to the sandbox's PID namespace.
    bind_host_root_entries(newroot, mount_root)?;

    // Fresh isolated /tmp — prevents temp file leakage to/from host.
    let tmp_path = newroot.join("tmp");
    dirs(&[&tmp_path])?;
    mnt::tmpfs(&tmp_path, TMPFS_SIZE)?;
    debug!("fresh /tmp mounted in newroot");

    // Build /home/<user>/ with only XDG dirs exposed (RW bind mounts).
    // Everything else in the user's home is invisible.
    if let Some(base_dirs) = base_dirs {
        bind_xdg_dirs(newroot, base_dirs)?;
    }

    Ok(())
}

/// Host root paths that are handled separately and must not be
/// bind-mounted from the host during enumeration.
///
/// - `/tmp`, `/home` — replaced by the sandbox tmpfs and per-user XDG
///   bind mounts respectively.
///
/// The configured `mount_root` (default `/code`) is also skipped at runtime
/// — see [`bind_host_root_entries`]. Bind-mounting the host path would pull
/// in whatever occupies it (e.g., an outer nyne FUSE mount in nested
/// scenarios), shadowing or conflicting with the inner mount.
///
/// **`/proc` is intentionally NOT skipped** — the host `/proc` bind mount
/// is required for `/proc/self/uid_map` writes during `unshare_user_remap`.
/// The PID namespace init process (`init_main`) remounts `/proc` afterwards
/// to scope it to the sandbox's PID namespace.
const SKIP_ROOT_PATHS: &[&str] = &["/tmp", "/home"];

/// Enumerate host `/` and selectively bind-mount entries into `newroot`.
///
/// Bind mounts inherit the source's mount flags transparently — no
/// explicit RO/RW remount is applied. The host kernel already enforces
/// permissions (sysfs, procfs attributes, filesystem ACLs), and the
/// sandbox user has the same uid/gid as the host user after remap.
///
/// - Paths in [`SKIP_ROOT_PATHS`] and `mount_root` are skipped (handled elsewhere).
/// - Symlinks are recreated (preserves indirection, e.g., `/bin → /usr/bin`
///   on systems like NixOS where root-level symlinks are common).
/// - Regular files at `/` are skipped (unusual; no known use case).
///
/// **Caveat:** `/dev` and `/run` are bind-mounted with host flags.
/// `/run` exposes host runtime state (D-Bus socket, systemd sockets).
/// A more restrictive approach (e.g., only binding `/run/user/<uid>`)
/// is on the roadmap.
fn bind_host_root_entries(newroot: &Path, mount_root: &Path) -> Result<()> {
    let entries = fs::read_dir("/").wrap_err("enumerating host root")?;

    for entry in entries {
        let entry = entry.wrap_err("reading host root entry")?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let source = entry.path();

        if SKIP_ROOT_PATHS.iter().any(|p| Path::new(p) == source) || source == mount_root {
            continue;
        }

        let target = newroot.join(&name);
        // Does NOT follow symlinks — returns the entry's own type.
        let file_type = entry
            .file_type()
            .wrap_err_with(|| format!("stat {}", source.display()))?;

        if file_type.is_symlink() {
            let link_target = fs::read_link(&source).wrap_err_with(|| format!("readlink {}", source.display()))?;
            symlink(&link_target, &target)
                .wrap_err_with(|| format!("symlink {} → {}", target.display(), link_target.display()))?;
            debug!(name = %name_str, target = %link_target.display(), "symlink recreated");
        } else if file_type.is_dir() {
            dirs(&[&target])?;
            mnt::bind(&source, &target)?;
            debug!(name = %name_str, "host dir bind-mounted");
        }
        // Regular files at root level are unusual — skip silently.
    }

    Ok(())
}

/// Bind-mount only the user's XDG directories into the sandbox home.
///
/// Creates `/home/<user>/` on the newroot tmpfs and bind-mounts each
/// XDG directory (config, cache, data, state) read-write from the host.
/// Nothing else from the user's home is visible.
fn bind_xdg_dirs(newroot: &Path, base_dirs: &BaseDirs) -> Result<()> {
    let xdg_dirs = collect_xdg_dirs(base_dirs);
    if xdg_dirs.is_empty() {
        return Ok(());
    }

    // Create the home directory structure on the tmpfs.
    let home = base_dirs.home_dir();
    let home_in_newroot = newroot.join(home.relative_to(Path::new("/")));
    dirs(&[&home_in_newroot])?;

    for xdg_dir in &xdg_dirs {
        let rel = xdg_dir.relative_to(Path::new("/"));
        let dst = newroot.join(rel);
        dirs(&[&dst])?;
        mnt::bind(xdg_dir, &dst)?;
        info!(dir = %xdg_dir.display(), "XDG dir bind-mounted (RW)");
    }

    Ok(())
}

/// Apply user-configured bind mounts into the new root before pivot.
///
/// Must be called before `pivot_root` — the source paths reference the
/// host filesystem which becomes inaccessible after pivot. For each bind
/// mount, creates the appropriate mount point (directory or empty file,
/// matching the source type), performs the recursive bind, and optionally
/// remounts with user-specified flags (e.g., read-only).
fn apply_bind_mounts(newroot: &Path, bind_mounts: &[BindMount]) -> Result<()> {
    for bm in bind_mounts {
        let dst = newroot.join(bm.target.relative_to(Path::new("/")));

        // Create the mount point: directory for directory sources,
        // empty file for file sources. Bind mounts require the target
        // type to match the source type.
        if bm.source.is_dir() {
            dirs(&[&dst])?;
        } else {
            if let Some(parent) = dst.parent() {
                dirs(&[parent])?;
            }
            File::create(&dst).wrap_err_with(|| format!("creating mount point at {}", dst.display()))?;
        }

        mnt::bind(&bm.source, &dst)?;

        if let Some(flags) = bm.mount_flags() {
            mnt::remount(&dst, flags)?;
        }

        info!(
            source = %bm.source.display(),
            target = %bm.target.display(),
            flags = ?bm.flags,
            "user bind mount applied",
        );
    }
    Ok(())
}

/// Create the upper/work dirs and mount overlayfs at `target`.
///
/// `upper` holds captured writes, `work` is kernel scratch space — both
/// must live on the same filesystem and both are created with
/// `create_dir_all` so callers can pass fresh paths.
fn mount_overlay_layers(target: &Path, lower: &Path, upper: &Path, work: &Path) -> Result<()> {
    dirs(&[upper, work])?;
    mnt::overlay(target, lower, upper, work)
}

/// Create multiple directories (with intermediate parents).
fn dirs(dir_paths: &[&Path]) -> Result<()> {
    for path in dir_paths {
        fs::create_dir_all(path).wrap_err_with(|| format!("creating {}", path.display()))?;
    }
    Ok(())
}

/// Collect deduplicated XDG base directories that exist on disk.
///
/// Gathers config, cache, data, `data_local`, and state directories from
/// the `directories` crate. Deduplication is necessary because `data_dir()`
/// and `data_local_dir()` resolve to the same path on Linux
/// (`$XDG_DATA_HOME`). Only directories that actually exist are returned,
/// avoiding mount failures on missing paths.
fn collect_xdg_dirs(base_dirs: &BaseDirs) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    let candidates = [
        Some(base_dirs.config_dir()),
        Some(base_dirs.cache_dir()),
        Some(base_dirs.data_dir()),
        Some(base_dirs.data_local_dir()),
        base_dirs.state_dir(),
    ];
    for dir in candidates.into_iter().flatten() {
        // data_dir() and data_local_dir() can be the same path on Linux.
        if dir.is_dir() && !dirs.iter().any(|d| d == dir) {
            dirs.push(dir.to_path_buf());
        }
    }

    dirs
}
