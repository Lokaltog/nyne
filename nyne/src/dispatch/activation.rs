//! Shared context provided to providers during plugin activation.
//!
//! [`ActivationContext`] is the single shared object that lives for the entire
//! lifetime of a mount session. Plugins insert domain services (git, syntax,
//! LSP, etc.) into it during the activation phase, and providers clone an
//! `Arc<ActivationContext>` to access those services at request time.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anymap2::SendSyncAnyMap;

use crate::config::NyneConfig;
use crate::process::Spawner;
use crate::router::Filesystem;

/// Shared context provided to all providers during activation.
///
/// Created once per mount during the plugin activation phase. Plugins
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
    /// access — the daemon uses `source_root` for I/O.
    root: PathBuf,
    /// Source root — the daemon's internal working directory.
    /// All filesystem access during FUSE callbacks goes through this
    /// path, which may be an overlayfs merged dir or a direct bind mount.
    source_root: PathBuf,
    /// Filesystem access abstraction.
    fs: Arc<dyn Filesystem>,
    /// Process spawner with env isolation.
    spawner: Arc<Spawner>,
    /// Full configuration, stored for provider access.
    config: Arc<NyneConfig>,
    /// Display root with trailing slash — precomputed for path stripping.
    root_prefix: String,
    /// Plugin-provided services, keyed by type.
    extensions: SendSyncAnyMap,
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
        source_root: PathBuf,
        fs: Arc<dyn Filesystem>,
        config: Arc<NyneConfig>,
        spawner: Arc<Spawner>,
    ) -> Self {
        let mut root_prefix = root.display().to_string();
        root_prefix.push('/');
        Self {
            host_root,
            root,
            source_root,
            fs,
            spawner,
            config,
            root_prefix,
            extensions: SendSyncAnyMap::new(),
        }
    }

    /// The display root — the path the user sees as the project root.
    pub fn root(&self) -> &Path { &self.root }

    /// The source root — the daemon's internal working directory.
    pub fn source_root(&self) -> &Path { &self.source_root }

    /// The original project path on the host filesystem.
    pub fn host_root(&self) -> &Path { &self.host_root }

    /// Full configuration.
    pub fn config(&self) -> &NyneConfig { &self.config }

    /// Process spawner with env isolation.
    pub const fn spawner(&self) -> &Arc<Spawner> { &self.spawner }

    /// Filesystem access abstraction.
    pub fn fs(&self) -> &Arc<dyn Filesystem> { &self.fs }

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
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> { self.extensions.get::<T>() }

    /// Retrieve a mutable reference to a plugin-provided service by type.
    ///
    /// Only available during `activate()` when the context is `&mut`.
    pub fn get_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> { self.extensions.get_mut::<T>() }

    /// Get a mutable reference to a service, inserting the default if absent.
    ///
    /// Useful when multiple plugins contribute to a shared extension point
    /// and activation order is non-deterministic.
    pub fn get_or_insert_default<T: Send + Sync + Default + 'static>(&mut self) -> &mut T {
        self.extensions.entry::<T>().or_default()
    }

    /// Materialize a plugin-specific configuration struct from this context.
    ///
    /// Looks up the `plugin.<id>` TOML section in [`ActivationContext::config`]
    /// and deserializes it via [`PluginConfig::from_section`]. Returns the
    /// type's `Default` on a missing section or deserialization failure.
    pub fn plugin_config<C: crate::plugin::PluginConfig>(&self, id: &str) -> C {
        C::from_section(self.config().plugin.get(id))
    }

    /// Display root with trailing slash — for stripping absolute paths to relative.
    pub fn root_prefix(&self) -> &str { &self.root_prefix }

    /// Resolve a relative path to its absolute source filesystem path.
    ///
    /// This is the canonical way to get a path for daemon-side I/O.
    /// Prefer this over `source_root().join(rel)`.
    pub fn source_path(&self, rel: impl AsRef<Path>) -> PathBuf { self.source_root.join(rel) }
}
/// Define a typed extension trait on [`ActivationContext`].
///
/// Generates both the trait definition and `impl for ActivationContext`.
/// Each entry is `method_name -> Type` (read-only, returns `Option<&T>`)
/// or `mut method_name -> Type` (read/write via `get_or_insert_default`,
/// returns `&mut T`).
///
/// ```ignore
/// activation_context_ext! {
///     /// Typed accessors for source plugin services.
///     pub trait SourceContextExt {
///         /// The shared syntax registry.
///         syntax_registry -> Arc<SyntaxRegistry>,
///         /// Mutable access to source extensions.
///         mut source_extensions_mut -> SourceExtensions,
///     }
/// }
/// ```
#[macro_export]
macro_rules! activation_context_ext {
    // Entry point: parse the full trait and delegate each entry to @entry.
    (
        $(#[$trait_meta:meta])*
        $vis:vis trait $Trait:ident {
            $($body:tt)*
        }
    ) => {
        $crate::activation_context_ext!(@collect
            meta: [$(#[$trait_meta])*]
            vis: [$vis]
            trait_name: [$Trait]
            entries: []
            rest: [$($body)*]
        );
    };

    // Collect: read-only entry.
    (@collect
        meta: [$($trait_meta:tt)*]
        vis: [$vis:vis]
        trait_name: [$Trait:ident]
        entries: [$($entries:tt)*]
        rest: [$(#[$meta:meta])* $method:ident -> $T:ty, $($rest:tt)*]
    ) => {
        $crate::activation_context_ext!(@collect
            meta: [$($trait_meta)*]
            vis: [$vis]
            trait_name: [$Trait]
            entries: [$($entries)* { read, $(#[$meta])*, $method, $T }]
            rest: [$($rest)*]
        );
    };

    // Collect: mutable entry.
    (@collect
        meta: [$($trait_meta:tt)*]
        vis: [$vis:vis]
        trait_name: [$Trait:ident]
        entries: [$($entries:tt)*]
        rest: [$(#[$meta:meta])* mut $method:ident -> $T:ty, $($rest:tt)*]
    ) => {
        $crate::activation_context_ext!(@collect
            meta: [$($trait_meta)*]
            vis: [$vis]
            trait_name: [$Trait]
            entries: [$($entries)* { write, $(#[$meta])*, $method, $T }]
            rest: [$($rest)*]
        );
    };

    // Emit: all entries collected, generate trait + impl.
    (@collect
        meta: [$($trait_meta:tt)*]
        vis: [$vis:vis]
        trait_name: [$Trait:ident]
        entries: [$($entries:tt)*]
        rest: []
    ) => {
        $($trait_meta)*
        $vis trait $Trait {
            $($crate::activation_context_ext!(@sig $entries);)*
        }

        impl $Trait for $crate::dispatch::activation::ActivationContext {
            $($crate::activation_context_ext!(@body $entries);)*
        }
    };

    // Sig/body for read-only.
    (@sig { read, $(#[$meta:meta])*, $method:ident, $T:ty }) => {
        $(#[$meta])* fn $method(&self) -> Option<&$T>;
    };
    (@body { read, $(#[$meta:meta])*, $method:ident, $T:ty }) => {
        fn $method(&self) -> Option<&$T> { self.get() }
    };

    // Sig/body for mutable.
    (@sig { write, $(#[$meta:meta])*, $method:ident, $T:ty }) => {
        $(#[$meta])* fn $method(&mut self) -> &mut $T;
    };
    (@body { write, $(#[$meta:meta])*, $method:ident, $T:ty }) => {
        fn $method(&mut self) -> &mut $T { self.get_or_insert_default() }
    };
}
