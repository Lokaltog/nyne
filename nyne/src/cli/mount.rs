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

/// Number of FUSE handler threads.
const FUSE_THREADS: usize = 4;

/// Arguments for the `mount` subcommand.
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

/// A parsed mount spec: optional explicit ID + path.
#[derive(Debug, Clone)]
pub struct MountSpec {
    explicit_id: Option<String>,
    path: PathBuf,
}

impl FromStr for MountSpec {
    type Err = Infallible;

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
/// When dropped, the FUSE session unmounts, the watcher stops, and
/// the control socket is cleaned up.
#[allow(dead_code)]
struct SessionGuard {
    _session: fuser::BackgroundSession,
    _watcher: FsWatcher,
    _control_server: Option<sandbox::control::ControlServer>,
}

/// Run the mount subcommand: mount one or more directories as FUSE filesystems.
pub fn run(args: &MountArgs) -> Result<()> {
    let nyne_config = NyneConfig::load()?;
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

    // Print the mount map.
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
    for (id, path) in &mounts {
        term.write_line(&format!("  {}  →  {}", style(path.display()).green(), style(id).cyan(),))?;
    }
    term.write_line(&format!(
        "\n{}",
        style("To attach: nyne attach <id> -- <command>\nTo list:   nyne list").dim()
    ))?;

    // Build mount entries and launch all daemons.
    let entries: Vec<_> = mounts
        .into_iter()
        .map(|(id, path)| {
            let persist_root = match storage_strategy {
                StorageStrategy::Passthrough => None,
                _ => Some(sandbox::resolve_persist_root(None)?),
            };

            let config = sandbox::MountConfig::new(sandbox::MountEntry { path })?;

            let session_id = id.clone();
            let nyne_config = nyne_config.clone();
            let mount_fn: sandbox::MountFn = Box::new(move |mount_path| {
                build_fuse_session(
                    mount_path,
                    &session_id,
                    &nyne_config,
                    persist_root.as_deref(),
                    storage_strategy,
                )
            });

            Ok((config, id, mount_fn))
        })
        .collect::<Result<_>>()?;

    sandbox::run_mounts(entries)
}

/// Build the FUSE session: prepare storage, activate providers, mount FUSE, and start the control server.
///
/// Called from inside the sandbox namespace — `mount_path` is the bind-mounted
/// project directory visible to the daemon.
fn build_fuse_session(
    mount_path: &Path,
    session_id: &SessionId,
    nyne_config: &NyneConfig,
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
    let git_dir_component = activation_ctx.get::<GitDirName>().and_then(|g| g.0.clone());
    let path_filter = PathFilter::build(&storage_root, git_dir_component.clone());

    // Build router and FUSE handler.
    let events: Arc<BufferedEventSink> = Arc::new(BufferedEventSink::new());
    let router = Arc::new(Router::new(registry, real_fs, events, path_filter));

    // Build visibility map: config defaults + plugin contributions → name-based
    // rules (all mapped to `None` / passthrough). Shared with the control server
    // so `SetVisibility` requests from `nyne attach` take effect immediately.
    let plugin_processes = activation_ctx
        .get::<PassthroughProcesses>()
        .map_or_else(Vec::new, |p| p.0.clone());
    let name_rules = nyne_config
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
