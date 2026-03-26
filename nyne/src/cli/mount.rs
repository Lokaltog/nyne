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
use tracing::{debug, info, warn};

use super::output::{self, style};
use crate::config::{NyneConfig, StorageStrategy};
use crate::dispatch::ScriptRegistry;
use crate::dispatch::path_filter::PathFilter;
use crate::fuse::VisibilityMap;
use crate::process::Spawner;
use crate::session::{self, SessionId, SessionRegistry};
use crate::types::{GitDirName, PassthroughProcesses, ProcessVisibility};
use crate::watcher::FsWatcher;
use crate::{AsyncNotifier, BufferedEventSink, FuseNotifier, NyneFs, OsFs, ProviderRegistry, Router, sandbox};

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
    /// Optional `id:` prefix sets the session ID.
    ///
    /// Examples:
    ///   nyne mount
    ///   nyne mount /path/to/project
    ///   nyne mount <myid:/path/to/project>
    ///   nyne mount /path/a /path/b
    pub paths: Vec<MountSpec>,
}

/// A parsed mount specification: optional explicit session ID + directory path.
///
/// Parsed from the CLI argument string via [`FromStr`]. The format is either
/// a bare path (`/path/to/project`) or an `id:path` pair (`myid:/path/to/project`).
/// When no explicit ID is given, one is derived from the directory name at
/// mount time.
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

/// Owns the FUSE session, filesystem watcher, and control IPC server.
///
/// All three resources are kept alive for the lifetime of the mount. When
/// this guard is dropped (on `SIGINT`, `SIGTERM`, or normal exit), the FUSE
/// session unmounts the overlay, the watcher stops monitoring for changes,
/// and the control socket is cleaned up. The `#[allow(dead_code)]` is
/// intentional -- the fields are never read, only held for their drop side
/// effects.
#[allow(dead_code)]
struct SessionGuard {
    _session: fuser::BackgroundSession,
    _watcher: FsWatcher,
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
    let nyne_config = Arc::new(NyneConfig::load()?);
    let storage_strategy = nyne_config.repository.storage_strategy;

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

    // Build mount entries and launch all daemons.
    let entries: Vec<_> = mounts
        .into_iter()
        .map(|(id, path)| {
            let persist_root = match storage_strategy {
                StorageStrategy::Passthrough => None,
                _ => Some(sandbox::resolve_persist_root(None)?),
            };

            let config = sandbox::DaemonConfig::new(sandbox::MountEntry { path })?;

            let session_id = id.clone();
            let nyne_config = Arc::clone(&nyne_config);
            let mount_fn: sandbox::MountFn = Box::new(move |mount_path| {
                build_fuse_session(
                    mount_path,
                    &session_id,
                    Arc::clone(&nyne_config),
                    persist_root.as_deref(),
                    storage_strategy,
                )
            });

            Ok((config, id, mount_fn))
        })
        .collect::<Result<_>>()?;

    sandbox::run_mounts(entries)
}

/// Print the mount plan to the terminal before launching daemons.
///
/// Shows each path and its assigned session ID so the user can verify
/// the mapping before the (potentially long-running) mount begins. Also
/// prints a hint about `nyne attach` and `nyne list` for discoverability.
fn print_mount_plan(mounts: &[(SessionId, PathBuf)]) -> Result<()> {
    let term = output::term();
    term.write_line(&format!(
        "{}\n",
        style(format!(
            "Mounting {} path{}:",
            mounts.len(),
            if mounts.len() == 1 { "" } else { "s" }
        ))
        .bold()
    ))?;
    for (id, path) in mounts {
        term.write_line(&format!("  {}  →  {}", style(path.display()).green(), style(id).cyan(),))?;
    }
    term.write_line(&format!(
        "\n{}",
        style("To attach: nyne attach <id> -- <command>\nTo list:   nyne list").dim()
    ))?;
    Ok(())
}

/// Build and start a FUSE session with all supporting infrastructure.
///
/// Called from inside the sandbox namespace after fork -- `mount_path` is the
/// bind-mounted project directory visible to the daemon. This function is the
/// heart of the mount lifecycle:
///
/// 1. **Storage**: prepare the backing store (passthrough bind or clone overlay)
///    per the configured [`StorageStrategy`].
/// 2. **Providers**: activate all linked plugins via [`ProviderRegistry`], which
///    populates the `TypeMap` with shared services.
/// 3. **Scripts**: build the script registry from provider-contributed scripts.
/// 4. **Router**: assemble the dispatch router that maps FUSE paths to provider
///    content, including path filters for `.git/` exclusion.
/// 5. **Visibility**: build the per-process visibility map from config defaults
///    and plugin contributions (e.g., LSP servers needing passthrough).
/// 6. **Control server**: start the Unix socket IPC server for `nyne ctl`/`exec`.
/// 7. **FUSE mount**: spawn the `fuser` background session with [`FUSE_THREADS`]
///    handler threads.
/// 8. **Watcher**: start the filesystem watcher for change-driven invalidation.
///
/// Returns a boxed [`SessionGuard`] that keeps everything alive until dropped.
///
/// # Errors
///
/// Returns an error if any step fails (storage prep, FUSE mount, etc.).
/// Control server failures are non-fatal -- logged as warnings.
fn build_fuse_session(
    mount_path: &Path,
    session_id: &SessionId,
    nyne_config: Arc<NyneConfig>,
    persist_root: Option<&Path>,
    storage_strategy: StorageStrategy,
) -> Result<Box<dyn Send>> {
    let mount_path = mount_path.to_path_buf();

    // Prepare the project backing path (passthrough bind-mount or
    // clone-backed overlay, depending on strategy).
    let storage_root = sandbox::prepare_project_storage(&mount_path, persist_root, storage_strategy)?;
    debug!(
        mount = %mount_path.display(),
        storage = %storage_root.display(),
        strategy = %storage_strategy,
        "project storage prepared"
    );

    // Build real filesystem and provider registry.
    let real_fs: Arc<dyn crate::RealFs> = Arc::new(OsFs::new(storage_root.clone()));
    let display_root = Path::new(sandbox::SANDBOX_CODE);
    let spawner = Arc::new(Spawner::new());
    let (provider_registry, activation_ctx) = ProviderRegistry::default_for(
        &mount_path,
        display_root,
        &storage_root,
        Arc::clone(&real_fs),
        nyne_config,
        spawner,
    );
    let registry = Arc::new(provider_registry);
    info!(
        path = %mount_path.display(),
        active_count = registry.active_providers().len(),
        "providers activated"
    );

    // Build script registry.
    let script_registry = Arc::new(ScriptRegistry::new(&activation_ctx));

    // Git directory name comes from the git plugin via TypeMap.
    // If no git plugin is active, falls back to None (no git dir filtering).
    let git_dir_component = activation_ctx.get::<GitDirName>().map(|g| g.0.clone());
    let path_filter = PathFilter::build(&storage_root, git_dir_component.clone());

    // Build router and FUSE handler.
    let events: Arc<BufferedEventSink> = Arc::new(BufferedEventSink::new());
    let router = Arc::new(Router::new(registry, real_fs, events, path_filter));

    // Build visibility map: config defaults + plugin contributions → name-based
    // rules (all mapped to `None` / passthrough). Shared with the control server
    // so `SetVisibility` requests from `nyne attach` take effect immediately.
    let plugin_processes = activation_ctx
        .get::<PassthroughProcesses>()
        .map_or_else(Vec::new, |p| p.as_slice().to_vec());
    let name_rules = activation_ctx
        .config()
        .passthrough_processes
        .iter()
        .cloned()
        .chain(plugin_processes)
        .map(|name| (name, ProcessVisibility::None));
    let visibility = Arc::new(VisibilityMap::new(name_rules).with_cgroup_tracking());

    // Ensure session dir exists inside the daemon's mount namespace.
    // This is intentionally separate from the `ensure_session_dir()` call
    // in `run()` — the daemon runs in a new mount namespace after fork,
    // so the parent's session dir may not be visible here.
    session::ensure_session_dir()?;
    let control_server = match session::control_socket(session_id.as_str()) {
        Ok(socket_path) => {
            let server = sandbox::control::start_server(
                &socket_path,
                Arc::clone(&script_registry),
                Arc::clone(&activation_ctx),
                Arc::clone(&visibility),
            )?;
            Some(server)
        }
        Err(e) => {
            warn!(error = %e, "could not determine control socket path — nyne exec will not be available");
            None
        }
    };

    let fs = NyneFs::new(Arc::clone(&router), visibility);

    // Mount FUSE.
    let mut fuse_config = fuser::Config::default();
    fuse_config.n_threads = Some(FUSE_THREADS);
    fuse_config.clone_fd = true;
    let session = fuser::spawn_mount2(fs, &mount_path, &fuse_config)
        .wrap_err_with(|| format!("mounting FUSE at {}", mount_path.display()))?;

    router.set_kernel_notifier(Box::new(AsyncNotifier::new(FuseNotifier::new(session.notifier()))));

    let watcher = FsWatcher::new(&storage_root, Arc::clone(&router), git_dir_component.as_deref())?;

    info!(path = %mount_path.display(), "FUSE session spawned");

    let guard = SessionGuard {
        _session: session,
        _watcher: watcher,
        _control_server: control_server,
    };
    Ok(Box::new(guard) as Box<dyn Send>)
}
