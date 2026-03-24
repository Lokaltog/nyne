use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::NyneConfig;
use crate::process::Spawner;
use crate::types::TypeMap;
use crate::types::real_fs::RealFs;

/// Shared context provided to all providers during activation.
///
/// Created once per mount by [`ProviderRegistry::default_for()`](crate::dispatch::registry::ProviderRegistry::default_for). Plugins
/// insert shared services via [`insert`](Self::insert) during activation,
/// then providers retrieve them via [`get`](Self::get) at request time.
///
/// Providers that need access to shared resources clone the
/// `Arc<ActivationContext>` and store it directly in their struct.
pub struct ActivationContext {
    /// Host root — the original project path on the host filesystem
    /// (before sandbox/overlay). Useful for display purposes (e.g.,
    /// statusline showing which project is mounted).
    host_root: PathBuf,
    /// Display root — the path the user sees as the project root inside
    /// the sandbox (`/code`). Used for template rendering, LSP path
    /// rewriting, and path-prefix computations. **Not** for filesystem
    /// access — the daemon uses `overlay_root` for I/O.
    root: PathBuf,
    /// Overlay merged path — the daemon's internal working directory.
    /// All filesystem access during FUSE callbacks goes through this
    /// path, which is a separate mount point from the FUSE overlay.
    overlay_root: PathBuf,
    /// Filesystem access abstraction.
    real_fs: Arc<dyn RealFs>,
    /// Process spawner with env isolation.
    spawner: Arc<Spawner>,
    /// Full configuration, stored for provider access.
    config: NyneConfig,
    /// Plugin-provided services, keyed by type.
    extensions: TypeMap,
}

/// Methods for building and querying the shared activation context.
impl ActivationContext {
    /// Build a new activation context with core fields only.
    ///
    /// Domain services (git, syntax, LSP, etc.) are inserted by plugins
    /// during activation via [`insert`](Self::insert).
    pub fn new(
        host_root: PathBuf,
        root: PathBuf,
        overlay_root: PathBuf,
        real_fs: Arc<dyn RealFs>,
        config: &NyneConfig,
        spawner: Arc<Spawner>,
    ) -> Self {
        Self {
            host_root,
            root,
            overlay_root,
            real_fs,
            spawner,
            config: config.clone(),
            extensions: TypeMap::new(),
        }
    }

    /// The display root — the path the user sees as the project root.
    pub fn root(&self) -> &Path { &self.root }

    /// The overlay merged path — the daemon's internal working directory.
    pub fn overlay_root(&self) -> &Path { &self.overlay_root }

    /// The original project path on the host filesystem.
    pub fn host_root(&self) -> &Path { &self.host_root }

    /// Full configuration.
    pub const fn config(&self) -> &NyneConfig { &self.config }

    /// Process spawner with env isolation.
    pub const fn spawner(&self) -> &Arc<Spawner> { &self.spawner }

    /// Filesystem access abstraction.
    pub fn real_fs(&self) -> &Arc<dyn RealFs> { &self.real_fs }

    /// Retrieve a plugin's configuration section (`[plugin.<id>]`).
    ///
    /// Returns `None` if the section doesn't exist (plugin uses defaults).
    pub fn plugin_config(&self, id: &str) -> Option<&toml::Value> { self.config.plugin.get(id) }

    /// Insert a service into the plugin extension map.
    ///
    /// Called by plugins during [`Plugin::activate`](crate::plugin::Plugin::activate).
    /// Services are keyed by their concrete type — insert `Arc<MyService>`
    /// and retrieve with `get::<Arc<MyService>>()`.
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) { self.extensions.insert(value); }

    /// Retrieve a plugin-provided service by type.
    ///
    /// Returns `None` if no plugin inserted this type. Providers should
    /// handle `None` gracefully (capability degradation).
    pub fn get<T: 'static>(&self) -> Option<&T> { self.extensions.get::<T>() }

    /// Display root with trailing slash — for stripping absolute paths to relative.
    pub fn root_prefix(&self) -> String { format!("{}/", self.root.display()) }
}
