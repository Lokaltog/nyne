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

use std::fs::File;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::{env, fs};

use color_eyre::eyre::{ContextCompat, Result, WrapErr};
use directories::BaseDirs;
use rustix::mount::MountFlags;
use rustix::process;
use tracing::{debug, info};

use super::{PROJECT_CLONERS, mnt, namespace, paths};
use crate::config::{BindMount, StorageStrategy};

/// Size limit for structural tmpfs mounts (newroot, /tmp).
const TMPFS_SIZE: &str = "2G";

/// Build the isolated sandbox root filesystem and mount FUSE at `/code`.
pub(super) fn setup(fuse_path: &Path, bind_mounts: &[BindMount]) -> Result<()> {
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

    let newroot = paths::newroot(process::getpid());

    build_newroot(&newroot, base_dirs.as_ref())?;

    // Bind-mount the current nyne binary so the sandbox uses the same
    // version that was invoked, not whatever is installed on the host.
    bind_self_exe(&newroot)?;

    // Apply user-configured bind mounts before pivot — sources are
    // still visible on the host filesystem at this point.
    apply_bind_mounts(&newroot, bind_mounts)?;

    mnt::pivot(&newroot)?;

    // Attach the FUSE clone at /code — the single entry point for
    // all project access inside the sandbox.
    let code = Path::new(paths::SANDBOX_CODE);
    dirs(&[code])?;
    mnt::attach(&fuse_fd, code)?;
    info!(path = %code.display(), "FUSE mounted at sandbox code path");

    // Lock down the root tmpfs. Mounts layered on top (FUSE, XDG
    // bind mounts, /dev, /run, /proc, /tmp) retain their own flags.
    mnt::remount(Path::new("/"), MountFlags::RDONLY)?;
    debug!("root remounted read-only");

    debug!("sandbox setup complete");

    Ok(())
}
/// Prepare the project backing path for the FUSE daemon.
///
/// The returned path is what the daemon uses for all filesystem I/O.
///
/// - **Passthrough**: bind-mounts the project directory RW at a stable
///   merged path. No clone, no overlayfs — writes hit the real filesystem.
/// - **Snapshot / Hardlink**: clones the project as an immutable lowerdir,
///   then mounts overlayfs at a separate merged path with a persistent
///   upperdir for write capture.
pub fn prepare_project_storage(path: &Path, persist_root: Option<&Path>, strategy: StorageStrategy) -> Result<PathBuf> {
    let pid = process::getpid();

    // Both strategies use the same merged path as the daemon's working directory.
    let merged = paths::merged_base(pid).join(paths::relative_to_root(path));
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
            let persist_root =
                persist_root.wrap_err("overlay strategies require a persist root for upperdir storage")?;
            let cache_dir = BaseDirs::new()
                .wrap_err("cannot determine XDG directories")?
                .cache_dir()
                .to_path_buf();

            // Clone project into a stable lower dir for use as overlayfs lowerdir.
            let lower = paths::lower_base(&cache_dir, pid).join(paths::relative_to_root(path));
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

            let slot = paths::persist_slot(persist_root, pid).join(paths::relative_to_root(path));
            mount_overlay_slot(&merged, &lower, &slot)?;
            info!(
                path = %path.display(),
                lower = %lower.display(),
                merged = %merged.display(),
                slot = %slot.display(),
                "project overlay mounted (clone-backed)"
            );
        }
    }

    Ok(merged)
}
/// Populate the new root with bind-mounted host entries, isolated `/tmp`, and XDG dirs.
fn build_newroot(newroot: &Path, base_dirs: Option<&BaseDirs>) -> Result<()> {
    dirs(&[newroot])?;
    mnt::tmpfs(newroot, TMPFS_SIZE)?;

    // Enumerate host root entries and selectively bind-mount them.
    // This replaces the previous rbind-all-then-override approach,
    // giving precise control over what's visible in the sandbox.
    // /proc is included — needed for /proc/self/uid_map writes during
    // user remap. The PID namespace init process remounts /proc after
    // fork to scope it to the sandbox's PID namespace.
    bind_host_root_entries(newroot)?;

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

/// Bind-mount the running nyne binary into the sandbox.
///
/// Resolves `/proc/self/exe` to find the invoking binary and mounts it
/// at `<newroot>/nyne/bin/nyne`. The corresponding `PATH` prepend happens
/// in [`super::init_main`] after user env vars are applied.
fn bind_self_exe(newroot: &Path) -> Result<()> {
    let exe = env::current_exe().wrap_err("resolving current executable")?;
    let bin_dir = newroot.join(paths::relative_to_root(Path::new(paths::NYNE_BIN_DIR)));
    let target = bin_dir.join("nyne");
    dirs(&[&bin_dir])?;
    File::create(&target).wrap_err_with(|| format!("creating mount point at {}", target.display()))?;
    mnt::bind(&exe, &target)?;
    debug!(exe = %exe.display(), "self binary bind-mounted");
    Ok(())
}

/// Top-level root entries that are handled separately and must not be
/// bind-mounted from the host during enumeration.
///
/// **`proc` is intentionally NOT skipped** — the host `/proc` bind mount
/// is required for `/proc/self/uid_map` writes during `unshare_user_remap`.
/// The PID namespace init process (`init_main`) remounts `/proc` afterwards
/// to scope it to the sandbox's PID namespace.
const SKIP_ROOT_ENTRIES: &[&str] = &["tmp", "home"];

/// Enumerate host `/` and selectively bind-mount entries into `newroot`.
///
/// Bind mounts inherit the source's mount flags transparently — no
/// explicit RO/RW remount is applied. The host kernel already enforces
/// permissions (sysfs, procfs attributes, filesystem ACLs), and the
/// sandbox user has the same uid/gid as the host user after remap.
///
/// - Directories in [`SKIP_ROOT_ENTRIES`] are skipped (handled elsewhere).
/// - Symlinks are recreated (preserves indirection, e.g., `/bin → /usr/bin`
///   on systems like NixOS where root-level symlinks are common).
/// - Regular files at `/` are skipped (unusual; no known use case).
///
/// **Caveat:** `/dev` and `/run` are bind-mounted with host flags.
/// `/run` exposes host runtime state (D-Bus socket, systemd sockets).
/// A more restrictive approach (e.g., only binding `/run/user/<uid>`)
/// is on the roadmap.
fn bind_host_root_entries(newroot: &Path) -> Result<()> {
    let entries = fs::read_dir("/").wrap_err("enumerating host root")?;

    for entry in entries {
        let entry = entry.wrap_err("reading host root entry")?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if SKIP_ROOT_ENTRIES.contains(&name_str.as_ref()) {
            continue;
        }

        let source = entry.path();
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
    let home_in_newroot = newroot.join(paths::relative_to_root(home));
    dirs(&[&home_in_newroot])?;

    for xdg_dir in &xdg_dirs {
        let rel = paths::relative_to_root(xdg_dir);
        let dst = newroot.join(rel);
        dirs(&[&dst])?;
        mnt::bind(xdg_dir, &dst)?;
        info!(dir = %xdg_dir.display(), "XDG dir bind-mounted (RW)");
    }

    Ok(())
}

/// Apply user-configured bind mounts into the new root before pivot.
fn apply_bind_mounts(newroot: &Path, bind_mounts: &[BindMount]) -> Result<()> {
    for bm in bind_mounts {
        let dst = newroot.join(paths::relative_to_root(&bm.target));

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

/// Create upper/work dirs in `slot` and mount overlayfs at `target`.
fn mount_overlay_slot(target: &Path, lower: &Path, slot: &Path) -> Result<()> {
    let upper = slot.join("upper");
    let work = slot.join("work");
    dirs(&[&upper, &work])?;
    mnt::overlay(target, lower, &upper, &work)
}

/// Create multiple directories (with intermediate parents).
fn dirs(dir_paths: &[&Path]) -> Result<()> {
    for path in dir_paths {
        fs::create_dir_all(path).wrap_err_with(|| format!("creating {}", path.display()))?;
    }
    Ok(())
}

/// Collect deduplicated XDG base directories that exist on disk.
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

/// Resolve the persist root for overlay upperdirs.
///
/// Uses the given path or falls back to `$XDG_CACHE_HOME/nyne/overlay/`.
pub fn resolve_persist_root(explicit: Option<&Path>) -> Result<PathBuf> {
    let root = if let Some(p) = explicit {
        p.to_path_buf()
    } else {
        let base = BaseDirs::new().wrap_err("cannot determine XDG directories")?;
        paths::default_persist_root(base.cache_dir())
    };
    fs::create_dir_all(&root).wrap_err_with(|| format!("creating persist root at {}", root.display()))?;
    debug!(path = %root.display(), "overlay persist root resolved");
    Ok(root)
}
