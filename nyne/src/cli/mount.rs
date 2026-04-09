//! `nyne mount` -- start FUSE daemon(s) for one or more directories.
//!
//! This is the primary entry point for running nyne. It resolves mount specs
//! (paths with optional session ID prefixes), prepares project storage per the
//! configured [`StorageStrategy`], builds the FUSE filesystem with all active
//! providers, and enters a sandboxed mount namespace. Each mount gets its own
//! FUSE session, filesystem watcher, and control server, all held alive by a
//! [`SessionGuard`] until the process is interrupted.

use std::convert::Infallible;
use std::env;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use clap::Args;
use color_eyre::eyre::{Result, WrapErr, ensure, eyre};
use tracing::info;

use super::output::{self, style};
use crate::config::{NyneConfig, StorageStrategy};
use crate::dispatch::activation::ActivationContext;
use crate::dispatch::{ControlRegistry, ScriptRegistry};
use crate::fuse::FuseFilesystem;
use crate::fuse::notify::{AsyncNotifier, FuseNotifier};
use crate::process::Spawner;
use crate::router::fs::os::OsFilesystem;
use crate::router::{Chain, Filesystem};
use crate::session::{self, SessionId, SessionRegistry};
use crate::watcher::{FsWatcher, WatcherBackend};
use crate::{plugin, procfs, sandbox};

/// Number of FUSE handler threads per mount.
///
/// Each thread can handle one FUSE request concurrently. Four threads
/// balance throughput against resource usage -- enough to avoid blocking
/// on parallel `readdir`/`getattr` bursts from shells and editors, without
/// over-committing on single-project mounts.
const FUSE_THREADS: usize = 4;

/// Arguments for the `mount` subcommand.
///
/// Accepts zero or more mount specs. When none are provided, the current
/// working directory is mounted with an auto-generated session ID. Multiple
/// specs can be given to mount several projects in a single daemon invocation.
#[derive(Debug, Args)]
pub struct MountArgs {
    /// Directories to mount. Defaults to the current directory if omitted.
    ///
    /// Optional `id:` prefix sets the session ID. The prefix is recognized only
    /// when it contains no `/` and both sides of the colon are non-empty, so
    /// absolute paths like `/home/user` are never misparsed. If a directory name
    /// contains a colon, use an absolute or `./`-relative path to avoid ambiguity.
    ///
    /// Examples:
    ///   nyne mount
    ///   nyne mount /path/to/project
    ///   nyne mount <myid:/path/to/project>
    ///   nyne mount /path/a /path/b
    pub paths: Vec<MountSpec>,

    /// Override the repository storage strategy from the config file.
    ///
    /// Accepts `passthrough`, `snapshot`, or `hardlink`. See [`StorageStrategy`]
    /// for variant descriptions.
    #[arg(long)]
    pub storage_strategy: Option<StorageStrategy>,
}

/// A parsed mount specification: optional explicit session ID + directory path.
///
/// Parsed from the CLI argument string via [`FromStr`]. The format is either
/// a bare path (`/path/to/project`) or an `id:path` pair (`myid:/path/to/project`).
/// When no explicit ID is given, one is derived from the directory name at
/// mount time.
///
/// # `id:` prefix heuristic
///
/// The parser splits on the *first* colon and treats the left side as a session
/// ID only when **all** of the following hold:
///
/// - The prefix is non-empty.
/// - The prefix contains no `/` (avoids misinterpreting absolute paths like
///   `/home/user`).
/// - The remainder after the colon is non-empty.
///
/// Everything else is treated as a bare path. This means:
///
/// - Windows-style paths (`C:\Users\...`) where the drive letter has no `/`
///   will be misparsed as `id = "C"`, `path = "\Users\..."`. This is harmless
///   on the Linux-only platforms nyne supports.
/// - Paths containing a colon in a directory name (e.g., `my:project/src`)
///   will be split: `id = "my"`, `path = "project/src"`. Use `./<path>` or an
///   absolute path to avoid ambiguity.
/// - A bare colon (`:`) or trailing colon (`foo:`) yields a bare path (no ID)
///   because one side of the split is empty.
#[derive(Debug, Clone)]
pub struct MountSpec {
    explicit_id: Option<String>,
    path: PathBuf,
}

impl FromStr for MountSpec {
    type Err = Infallible;

    /// Parse a mount spec from a CLI argument string.
    ///
    /// Recognizes the `id:path` format only when the prefix contains no `/`
    /// and the remainder is non-empty -- this avoids misinterpreting absolute
    /// paths like `/home/user` as having an `id` of nothing and a path of
    /// `home/user`. Parsing is infallible because any string that doesn't
    /// match the `id:path` pattern is treated as a bare path.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some((prefix, rest)) = s.split_once(':')
            && !prefix.is_empty()
            && !prefix.contains('/')
            && !rest.is_empty()
        {
            return Ok(Self {
                explicit_id: Some(prefix.to_owned()),
                path: PathBuf::from(rest),
            });
        }
        Ok(Self {
            explicit_id: None,
            path: PathBuf::from(s),
        })
    }
}

/// Owns the FUSE session and supporting infrastructure.
///
/// All resources are kept alive for the lifetime of the mount. When
/// this guard is dropped, the FUSE session unmounts the overlay and
/// supporting services are cleaned up.
#[allow(dead_code)]
struct SessionGuard {
    _session: fuser::BackgroundSession,
    watcher: FsWatcher,
    _control_server: Option<sandbox::control::Server>,
}

/// Run the mount subcommand: mount one or more directories as FUSE filesystems.
///
/// Orchestrates the full mount lifecycle:
/// 1. Load configuration and scan for existing sessions (to detect ID conflicts).
/// 2. Resolve each mount spec to a canonical path + unique session ID.
/// 3. Print the mount plan so the user sees what will happen.
/// 4. Build mount entries with closures that construct FUSE sessions inside
///    the sandbox namespace.
/// 5. Delegate to [`sandbox::run_mounts`] which forks, enters namespaces,
///    and calls the mount closures.
///
/// The function blocks until the daemon is interrupted. Each mount gets its
/// own FUSE session and control server, but all share a single process.
///
/// # Errors
///
/// Returns an error if config loading fails, paths are invalid, session IDs
/// conflict, or sandbox/FUSE setup fails.
pub fn run(args: &MountArgs) -> Result<()> {
    let plugins = plugin::instantiate();

    // Use first explicit path or CWD as the project root for config layering.
    let project_root = args
        .paths
        .first()
        .map(|s| s.path.clone())
        .map(|p| p.canonicalize().unwrap_or(p))
        .or_else(|| env::current_dir().ok());

    let mut nyne_config = NyneConfig::load(&plugins, project_root.as_deref())?;
    if let Some(strategy) = args.storage_strategy {
        nyne_config.repository.storage_strategy = strategy;
    }
    let storage_strategy = nyne_config.repository.storage_strategy;
    let nyne_config = Arc::new(nyne_config);

    // Ensure session directory exists before scanning.
    session::ensure_session_dir()?;
    let registry = SessionRegistry::scan()?;

    // Default to CWD if no paths provided.
    let default_cwd;
    let specs: &[MountSpec] = if args.paths.is_empty() {
        default_cwd = vec![MountSpec {
            explicit_id: None,
            path: env::current_dir().wrap_err("resolving current directory")?,
        }];
        &default_cwd
    } else {
        &args.paths
    };

    let mut mounts: Vec<(SessionId, PathBuf)> = Vec::with_capacity(specs.len());

    for spec in specs {
        let path = spec
            .path
            .canonicalize()
            .wrap_err_with(|| format!("resolving path: {}", spec.path.display()))?;
        ensure!(path.is_dir(), "not a directory: {}", path.display());

        let id = match &spec.explicit_id {
            Some(explicit) => SessionId::from_explicit(explicit, &registry)?,
            None => SessionId::from_path(&path, &registry)?,
        };

        // Check for duplicate IDs within this invocation.
        if mounts
            .iter()
            .any(|(existing_id, _)| existing_id.as_str() == id.as_str())
        {
            return Err(eyre!(
                "duplicate session ID {id:?} — use explicit prefixes to disambiguate"
            ));
        }

        mounts.push((id, path));
    }

    print_mount_plan(&mounts)?;

    let state_root = nyne_config.sandbox.state_root.clone();

    // Build mount entries and launch all daemons.
    let entries: Vec<_> = mounts
        .into_iter()
        .map(|(id, path)| {
            let config = sandbox::DaemonConfig::new(sandbox::MountEntry { path })?;

            // Resolve the control socket path in the supervisor's user
            // namespace so that the daemon (in its own fresh user ns)
            // binds at the same location clients look up.
            session::ensure_session_dir()?;
            let control_socket_path = session::control_socket(id.as_str())?;
            let nyne_config = Arc::clone(&nyne_config);
            let state_root = state_root.clone();
            let mount_fn: sandbox::MountFn = Box::new(move |mount_path| {
                build_fuse_session(
                    mount_path,
                    &control_socket_path,
                    &nyne_config,
                    &state_root,
                    storage_strategy,
                )
            });

            Ok((config, id, mount_fn))
        })
        .collect::<Result<_>>()?;

    sandbox::run_mounts(entries, &state_root)
}

/// Print the mount plan to the terminal before launching daemons.
///
/// Shows each path and its assigned session ID so the user can verify
/// the mapping before the (potentially long-running) mount begins. Also
/// prints a hint about `nyne attach` and `nyne list` for discoverability.
fn print_mount_plan(mounts: &[(SessionId, PathBuf)]) -> Result<()> {
    use std::fmt::Write;

    let mut buf = String::new();
    let suffix = if mounts.len() == 1 { "" } else { "s" };
    writeln!(
        buf,
        "{}\n",
        style(format_args!("Mounting {} path{suffix}:", mounts.len())).bold()
    )?;
    for (id, path) in mounts {
        writeln!(buf, "  {}  →  {}", style(path.display()).green(), style(id).cyan())?;
    }
    write!(
        buf,
        "\n{}",
        style("To attach: nyne attach <id> -- <command>\nTo list:   nyne list").dim()
    )?;
    output::term().write_line(&buf)?;
    Ok(())
}

/// Build and start a FUSE session with all supporting infrastructure.
///
/// 1. Prepare project storage (passthrough bind or clone overlay).
/// 2. Activate plugins, collect router providers.
/// 3. Build the middleware chain.
/// 4. Construct `FuseFilesystem` and mount FUSE.
/// 5. Set kernel notifier, start filesystem watcher.
///
/// Returns a [`SessionGuard`] that keeps everything alive until dropped.
fn build_fuse_session(
    mount_path: &Path,
    control_socket_path: &Path,
    nyne_config: &Arc<NyneConfig>,
    state_root: &Path,
    storage_strategy: StorageStrategy,
) -> Result<Box<dyn Send>> {
    let mount_path = mount_path.to_path_buf();

    // Prepare the project backing path.
    let storage_root = sandbox::prepare_project_storage(&mount_path, state_root, storage_strategy)?;

    // Build filesystem and activation context.
    let fs_backend: Arc<dyn Filesystem> = Arc::new(OsFilesystem::new(&storage_root));
    let display_root = nyne_config.sandbox.mount_root.as_path();
    let spawner = Arc::new(Spawner::new());
    let mut activation_ctx = ActivationContext::new(
        mount_path.clone(),
        display_root.to_path_buf(),
        storage_root.clone(),
        Arc::clone(&fs_backend),
        Arc::clone(nyne_config),
        spawner,
    );

    // Activate plugins in dependency order.
    let plugins = plugin::instantiate();
    let plugins = plugin::sort_by_deps(plugins)?;
    for p in &plugins {
        p.activate(&mut activation_ctx)?;
    }

    let activation_ctx = Arc::new(activation_ctx);

    // Collect all plugin contributions in a single pass.
    let mut all_providers = Vec::new();
    let mut all_scripts = Vec::new();
    let mut all_commands = Vec::new();
    for p in &plugins {
        let c = p.contributions(&activation_ctx)?;
        all_providers.extend(c.providers);
        all_scripts.extend(c.scripts);
        all_commands.extend(c.control_commands);
    }

    let chain = Arc::new(Chain::build(all_providers).wrap_err("chain build failed")?);
    let script_registry = Arc::new(ScriptRegistry::from_entries(all_scripts));
    let control_registry = Arc::new(ControlRegistry::from_commands(all_commands));

    // Open the daemon's own user + mount namespace fds. Sent to attach
    // clients via SCM_RIGHTS so they can `setns` without resolving the
    // daemon's PID (which breaks across sibling PID namespaces).
    // `daemon_main` has already called `unshare_user_mount`, so
    // `/proc/self/ns/{user,mnt}` point at the daemon's namespaces.
    let ns = Arc::new(sandbox::Namespace {
        user: procfs::self_ns_fd("user")?,
        mnt: procfs::self_ns_fd("mnt")?,
    });

    // Control server. The socket path was resolved by the supervisor
    // before forking, so it reflects the supervisor's namespace view —
    // matching the session file location and what clients will look up.
    let handlers = sandbox::control::Handlers::new(
        Arc::clone(&script_registry),
        Arc::clone(&activation_ctx),
        control_registry,
        Arc::clone(&chain),
        ns,
    );
    let control_server = Some(sandbox::control::start_server(control_socket_path, handlers)?);

    // Build FuseFilesystem and mount.
    let fs = FuseFilesystem::new(Arc::clone(&chain), fs_backend);
    let notifier_slot = Arc::clone(fs.notifier());
    let inodes = Arc::clone(fs.inodes());
    let inline_writes = Arc::clone(fs.inline_writes());

    let mut fuse_config = fuser::Config::default();
    fuse_config.n_threads = Some(FUSE_THREADS);
    fuse_config.clone_fd = true;
    let fuse_session = fuser::spawn_mount2(fs, &mount_path, &fuse_config)
        .wrap_err_with(|| format!("mounting FUSE at {}", mount_path.display()))?;

    // Set the kernel notifier now that the FUSE session is running.
    notifier_slot
        .set(Box::new(AsyncNotifier::new(FuseNotifier::new(fuse_session.notifier()))))
        .ok();

    // Start filesystem watcher.
    let watcher_backend = WatcherBackend {
        chain,
        inodes,
        notifier: notifier_slot,
        inline_writes,
    };
    let watcher = FsWatcher::new(&storage_root, watcher_backend)?;

    info!(path = %mount_path.display(), "FUSE session spawned");

    Ok(Box::new(SessionGuard {
        _session: fuse_session,
        watcher,
        _control_server: control_server,
    }))
}
